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
    struct MyFetch<'a> {
        #[fetch(position())]
        pos: &'a Vec3,
        #[fetch(rotation().opt())]
        rot: Option<&'a Quat>,
        #[fetch(scale().opt_or(Vec3::ONE))]
        scale: &'a Vec3,
    }
}
