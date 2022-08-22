use flax::*;
use flax_derive::Fetch;
use glam::*;

#[test]
fn derive_fetch() {
    component! {
        position: Vec3 => [Debug],
        rotation: Quat => [Debug],
        scale: Vec3 => [Debug],
    }

    use glam::*;

    #[derive(Debug, Clone, Fetch)]
    struct MyFetch<'q> {
        #[fetch(position())]
        pos: &'q Vec3,
        #[fetch(rotation().opt())]
        rot: Option<&'q Quat>,
        #[fetch(scale().opt_or(Vec3::ONE))]
        scale: Vec3,
    }
}
