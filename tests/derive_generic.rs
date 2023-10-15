#[test]
#[cfg(feature = "derive")]
fn derive_fetch_generic() {
    flax::component! {
        position: Vec3 => [flax::Debuggable],
        rotation: Quat => [flax::Debuggable],
        scale: Vec3 => [flax::Debuggable],
    }

    use glam::*;

    use flax::{component::ComponentValue, Fetch, *};

    #[derive(Fetch)]
    #[fetch(item_derives = [Debug, PartialEq])]
    struct TransformQuery<V: ComponentValue> {
        pos: Component<V>,
        rot: Opt<Component<Quat>>,
        scale: Opt<Component<V>>,
    }

    let mut world = World::new();

    let id1 = Entity::builder()
        .set(position(), vec3(3.4, 2.4, 2.1))
        .spawn(&mut world);

    let id2 = Entity::builder()
        .set(position(), vec3(7.4, 9.2, 3.4))
        .set(rotation(), Quat::from_axis_angle(Vec3::Z, 1.0))
        .spawn(&mut world);

    let mut query = Query::new(TransformQuery {
        pos: position(),
        rot: rotation().opt(),
        scale: scale().opt(),
    });

    let mut query = query.borrow(&world);

    assert_eq!(
        query.get(id1),
        Ok(TransformQueryItem {
            pos: &vec3(3.4, 2.4, 2.1),
            rot: None,
            scale: None
        })
    );

    assert_eq!(
        query.get(id2),
        Ok(TransformQueryItem {
            pos: &vec3(7.4, 9.2, 3.4),
            rot: Some(&Quat::from_axis_angle(Vec3::Z, 1.0)),
            scale: None
        })
    );
}
