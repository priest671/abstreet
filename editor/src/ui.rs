use crate::colors::ColorScheme;
use abstutil;
//use cpuprofiler;
use crate::objects::{Ctx, RenderingHints, ID, ROOT_MENU};
use crate::render::{DrawMap, RenderOptions};
use crate::state::{DefaultUIState, PluginsPerMap, UIState};
use ezgui::{Canvas, Color, EventLoopMode, GfxCtx, Text, UserInput, BOTTOM_LEFT, GUI};
use kml;
use map_model::{BuildingID, LaneID, Map};
use piston::input::Key;
use serde_derive::{Deserialize, Serialize};
use sim;
use sim::{Sim, SimFlags, Tick};
use std::borrow::Borrow;
use std::collections::{HashMap, HashSet};
use std::process;

const MIN_ZOOM_FOR_MOUSEOVER: f64 = 4.0;

pub struct UI {
    // TODO Use generics instead
    state: Box<UIState>,
    canvas: Canvas,
    cs: ColorScheme,
}

impl GUI<RenderingHints> for UI {
    fn event(&mut self, mut input: UserInput) -> (EventLoopMode, RenderingHints) {
        let mut hints = RenderingHints {
            mode: EventLoopMode::InputOnly,
            osd: Text::new(),
            suppress_intersection_icon: None,
            color_crosswalks: HashMap::new(),
            hide_crosswalks: HashSet::new(),
            hide_turn_icons: HashSet::new(),
        };

        // First update the camera and handle zoom
        let old_zoom = self.canvas.cam_zoom;
        self.canvas.handle_event(&mut input);
        let new_zoom = self.canvas.cam_zoom;
        self.state.handle_zoom(old_zoom, new_zoom);

        // Always handle mouseover
        if old_zoom >= MIN_ZOOM_FOR_MOUSEOVER && new_zoom < MIN_ZOOM_FOR_MOUSEOVER {
            self.state.set_current_selection(None);
        }
        if !self.canvas.is_dragging()
            && input.get_moved_mouse().is_some()
            && new_zoom >= MIN_ZOOM_FOR_MOUSEOVER
        {
            self.state.set_current_selection(self.mouseover_something());
        }

        let mut recalculate_current_selection = false;
        self.state.event(
            &mut input,
            &mut hints,
            &mut recalculate_current_selection,
            &mut self.cs,
            &mut self.canvas,
        );
        if recalculate_current_selection {
            self.state.set_current_selection(self.mouseover_something());
        }

        // Can do this at any time.
        if input.unimportant_key_pressed(Key::Escape, ROOT_MENU, "quit") {
            self.save_editor_state();
            self.cs.save();
            info!("Saved color_scheme");
            //cpuprofiler::PROFILER.lock().unwrap().stop().unwrap();
            process::exit(0);
        }

        input.populate_osd(&mut hints.osd);

        (hints.mode, hints)
    }

    fn get_mut_canvas(&mut self) -> &mut Canvas {
        &mut self.canvas
    }

    fn draw(&self, g: &mut GfxCtx, hints: RenderingHints) {
        g.clear(self.cs.get_def("map background", Color::rgb(242, 239, 233)));

        let ctx = Ctx {
            cs: &self.cs,
            map: &self.state.primary().map,
            draw_map: &self.state.primary().draw_map,
            canvas: &self.canvas,
            sim: &self.state.primary().sim,
            hints: &hints,
        };

        let (statics, dynamics) = self.state.get_objects_onscreen(&self.canvas);
        for obj in statics
            .into_iter()
            .chain(dynamics.iter().map(|obj| Box::new(obj.borrow())))
        {
            let opts = RenderOptions {
                color: self.color_obj(obj.get_id(), &ctx),
                cam_zoom: self.canvas.cam_zoom,
                debug_mode: self.state.is_debug_mode_enabled(),
            };
            obj.draw(g, opts, &ctx);
        }

        self.state.draw(g, &ctx);

        self.canvas.draw_text(g, hints.osd, BOTTOM_LEFT);
    }

    fn dump_before_abort(&self) {
        self.state.dump_before_abort();
        self.save_editor_state();
    }
}

// All of the state that's bound to a specific map+edit has to live here.
// TODO How can we arrange the code so that we statically know that we don't pass anything from UI
// to something in PerMapUI?
pub struct PerMapUI {
    pub map: Map,
    pub draw_map: DrawMap,
    pub sim: Sim,

    pub current_selection: Option<ID>,
    pub current_flags: SimFlags,
}

impl PerMapUI {
    pub fn new(flags: SimFlags, kml: Option<String>, canvas: &Canvas) -> (PerMapUI, PluginsPerMap) {
        let mut timer = abstutil::Timer::new("setup PerMapUI");

        let (map, sim) = sim::load(flags.clone(), Some(Tick::from_seconds(30)), &mut timer);
        let extra_shapes: Vec<kml::ExtraShape> = if let Some(path) = kml {
            if path.ends_with(".kml") {
                kml::load(&path, &map.get_gps_bounds(), &mut timer)
                    .expect("Couldn't load extra KML shapes")
                    .shapes
            } else {
                let shapes: kml::ExtraShapes =
                    abstutil::read_binary(&path, &mut timer).expect("Couldn't load ExtraShapes");
                shapes.shapes
            }
        } else {
            Vec::new()
        };

        timer.start("draw_map");
        let draw_map = DrawMap::new(&map, extra_shapes, &mut timer);
        timer.stop("draw_map");

        let state = PerMapUI {
            map,
            draw_map,
            sim,
            current_selection: None,
            current_flags: flags,
        };
        let plugins = PluginsPerMap::new(&state, canvas, &mut timer);
        timer.done();
        (state, plugins)
    }
}

impl UI {
    pub fn new(flags: SimFlags, kml: Option<String>) -> UI {
        let canvas = Canvas::new();
        let state = Box::new(DefaultUIState::new(flags, kml, &canvas));

        let mut ui = UI {
            state,
            canvas,
            cs: ColorScheme::load().unwrap(),
        };

        match abstutil::read_json::<EditorState>("editor_state") {
            Ok(ref state) if ui.state.primary().map.get_name() == &state.map_name => {
                info!("Loaded previous editor_state");
                ui.canvas.cam_x = state.cam_x;
                ui.canvas.cam_y = state.cam_y;
                ui.canvas.cam_zoom = state.cam_zoom;
            }
            _ => {
                warn!("Couldn't load editor_state or it's for a different map, so just focusing on an arbitrary building");
                // TODO window_size isn't set yet, so this actually kinda breaks
                let focus_pt = ID::Building(BuildingID(0))
                    .canonical_point(
                        &ui.state.primary().map,
                        &ui.state.primary().sim,
                        &ui.state.primary().draw_map,
                    )
                    .or_else(|| {
                        ID::Lane(LaneID(0)).canonical_point(
                            &ui.state.primary().map,
                            &ui.state.primary().sim,
                            &ui.state.primary().draw_map,
                        )
                    })
                    .expect("Can't get canonical_point of BuildingID(0) or Road(0)");
                ui.canvas.center_on_map_pt(focus_pt);
            }
        }

        ui
    }

    fn mouseover_something(&self) -> Option<ID> {
        let pt = self.canvas.get_cursor_in_map_space();

        let (statics, dynamics) = self.state.get_objects_onscreen(&self.canvas);
        // Check front-to-back
        for obj in dynamics
            .iter()
            .map(|obj| Box::new(obj.borrow()))
            .chain(statics.into_iter().rev())
        {
            if obj.contains_pt(pt) {
                return Some(obj.get_id());
            }
        }

        None
    }

    fn color_obj(&self, id: ID, ctx: &Ctx) -> Option<Color> {
        self.state.color_obj(id, ctx)
    }

    fn save_editor_state(&self) {
        let state = EditorState {
            map_name: self.state.primary().map.get_name().clone(),
            cam_x: self.canvas.cam_x,
            cam_y: self.canvas.cam_y,
            cam_zoom: self.canvas.cam_zoom,
        };
        // TODO maybe make state line up with the map, so loading from a new map doesn't break
        abstutil::write_json("editor_state", &state).expect("Saving editor_state failed");
        info!("Saved editor_state");
    }
}

#[derive(Serialize, Deserialize, Debug)]
pub struct EditorState {
    pub map_name: String,
    pub cam_x: f64,
    pub cam_y: f64,
    pub cam_zoom: f64,
}
