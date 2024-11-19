#[test]
#[cfg(all(feature = "derive", feature = "rayon"))]
fn system_macro() {
    use flax::{
        component, system, CommandBuffer, Component, Entity, FetchExt, Query, QueryBorrow,
        Schedule, World,
    };

    component! {
        a: i32,
        b: String,
        c: i32,
        d: Vec<String>,
        my_type: MyType,
    }

    pub struct MyType;

    impl MyType {
        #[system(filter(d().with()))]
        pub fn method(self: &mut MyType, b: &String) {
            eprintln!("method: {b:?}")
        }

        #[system(with_cmd_mut)]
        pub fn method_with_cmd(self: &mut MyType, b: &String, cmd: &mut CommandBuffer) {
            let _ = b;
            let _ = cmd;
        }

        #[system(with_world, with_cmd_mut, with_query(Query::new(a())))]
        pub fn method_with_all_sides(
            self: &mut MyType,
            b: &String,
            world: &World,
            _cmd: &mut CommandBuffer,
            _query: &mut QueryBorrow<Component<i32>>,
        ) {
            let _ = b;
            eprintln!("world: {world:#?}")
        }
    }

    #[system(args(c_renamed = c().cloned()), par)]
    fn update(a: &i32, b: &mut String, c_renamed: i32, d: Option<&mut Vec<String>>) {
        *b = b.to_uppercase();
        eprintln!("{a} {b} {c_renamed} {d:?}");
    }

    #[system(par)]
    fn fallible(a: &mut i32) -> anyhow::Result<()> {
        let _ = a;
        Ok(())
    }

    let mut world = World::new();

    Entity::builder()
        .set(a(), 5)
        .set(b(), "Foo".into())
        .set(c(), -4)
        .spawn(&mut world);

    Entity::builder()
        .set(a(), 7)
        .set(b(), "Bar".into())
        .set(c(), -4)
        .set(d(), vec!["A".into(), "B".into()])
        .set(my_type(), MyType)
        .spawn(&mut world);

    let mut schedule = Schedule::builder()
        .with_system(update_system())
        .with_system(fallible_system())
        .with_system(MyType::method_system())
        .with_system(MyType::method_with_all_sides_system())
        .build();

    schedule.execute_seq(&mut world).unwrap();
}
