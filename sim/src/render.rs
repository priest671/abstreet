use crate::{CarID, PedestrianID, Sim, VehicleType};
use geom::{Duration, PolyLine, Pt2D};
use map_model::{LaneType, Map, Trace, Traversable, TurnID};

// Intermediate structures so that sim and editor crates don't have a cyclic dependency.
#[derive(Clone)]
pub struct DrawPedestrianInput {
    pub id: PedestrianID,
    pub pos: Pt2D,
    pub waiting_for_turn: Option<TurnID>,
    pub preparing_bike: bool,
    pub on: Traversable,
}

#[derive(Clone)]
pub struct DrawCarInput {
    pub id: CarID,
    pub waiting_for_turn: Option<TurnID>,
    pub stopping_trace: Option<Trace>,
    pub status: CarStatus,
    // TODO This is definitely redundant
    pub vehicle_type: VehicleType,
    pub on: Traversable,

    // Starts at the BACK of the car. TODO Dedupe unused old stuff.
    pub body: PolyLine,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum CarStatus {
    Moving,
    Stuck,
    Parked,
    Debug,
}

// TODO Can we return borrows instead? Nice for time travel, not for main sim?
// actually good for main sim too; we're constantly calculating stuff while sim is paused
// otherwise? except we don't know what to calculate. maybe cache it?
pub trait GetDrawAgents {
    fn time(&self) -> Duration;
    fn get_draw_car(&self, id: CarID, map: &Map) -> Option<DrawCarInput>;
    fn get_draw_ped(&self, id: PedestrianID, map: &Map) -> Option<DrawPedestrianInput>;
    fn get_draw_cars(&self, on: Traversable, map: &Map) -> Vec<DrawCarInput>;
    fn get_draw_peds(&self, on: Traversable, map: &Map) -> Vec<DrawPedestrianInput>;
    fn get_all_draw_cars(&self, map: &Map) -> Vec<DrawCarInput>;
    fn get_all_draw_peds(&self, map: &Map) -> Vec<DrawPedestrianInput>;
}

impl GetDrawAgents for Sim {
    fn time(&self) -> Duration {
        self.time.as_time()
    }

    fn get_draw_car(&self, id: CarID, map: &Map) -> Option<DrawCarInput> {
        self.driving_state
            .get_draw_car(id, self.time, map)
            .or_else(|| self.parking_state.get_draw_car(id, map))
    }

    fn get_draw_ped(&self, id: PedestrianID, map: &Map) -> Option<DrawPedestrianInput> {
        self.walking_state.get_draw_ped(id, map, self.time)
    }

    fn get_draw_cars(&self, on: Traversable, map: &Map) -> Vec<DrawCarInput> {
        match on {
            Traversable::Lane(l) => match map.get_l(l).lane_type {
                LaneType::Driving | LaneType::Bus | LaneType::Biking => {
                    self.driving_state.get_draw_cars(on, self.time, map)
                }
                LaneType::Parking => self.parking_state.get_draw_cars(l, map),
                LaneType::Sidewalk => Vec::new(),
            },
            Traversable::Turn(_) => self.driving_state.get_draw_cars(on, self.time, map),
        }
    }

    fn get_draw_peds(&self, on: Traversable, map: &Map) -> Vec<DrawPedestrianInput> {
        self.walking_state.get_draw_peds(on, map, self.time)
    }

    fn get_all_draw_cars(&self, map: &Map) -> Vec<DrawCarInput> {
        let mut cars = self.driving_state.get_all_draw_cars(self.time, map);
        cars.extend(self.parking_state.get_all_draw_cars(map));
        cars
    }

    fn get_all_draw_peds(&self, map: &Map) -> Vec<DrawPedestrianInput> {
        self.walking_state.get_all_draw_peds(self.time, map)
    }
}
