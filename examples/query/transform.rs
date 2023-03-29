// use flax::{component, All, Component, OptOr, Query, QueryBorrow, System};

// fn main() {
//     component! {
//         position: f32,
//     }

//     let query = Query::new(OptOr::new(position(), 0.0));

//     let system = System::builder()
//         .with(query)
//         // Should be `OptOr<Component<f32>>`
//         .build(|query: QueryBorrow<Component<_>, All>| {});
// }
