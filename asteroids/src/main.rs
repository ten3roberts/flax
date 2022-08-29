use color_eyre::{
    eyre::{eyre, ContextCompat},
    owo_colors::colors::xterm::PoloBlue,
    Result,
};
use std::f32::consts::TAU;
use tracing_subscriber::{prelude::*, registry};

use flax::*;
use glam::*;
use macroquad::{
    color::hsl_to_rgb,
    prelude::{is_key_down, Color, KeyCode, BLACK, DARKPURPLE, GREEN},
    shapes::{draw_poly, draw_rectangle, draw_triangle},
    time::get_frame_time,
    window::{clear_background, next_frame, screen_height, screen_width},
};
use rand::{rngs::StdRng, Rng, SeedableRng};
use tracing_tree::HierarchicalLayer;

component! {
    position: Vec2 => [ Debug ],
    rotation: f32 => [ Debug ],

    asteroid: () => [ Debug ],
    player: () => [ Debug ],

    camera: Mat3,
    health: f32,
    color: Color,
    mass: f32,

    velocity: Vec2=> [ Debug ],
    angular_velocity: f32 => [ Debug ],

    shape: Shape,
    radius: f32,

    on_collision: Box<dyn Fn(EntityRef, Collision) + Send + Sync>,

}

#[derive(Debug, Clone)]
enum Shape {
    Polygon { radius: f32, sides: u8 },
    Triangle(Vec2, Vec2, Vec2),
}

impl Shape {
    pub fn draw(&self, view: &Mat3, pos: Vec2, rot: f32, color: Color) {
        match *self {
            Shape::Polygon { radius, sides } => {
                let pos = view.transform_point2(pos);
                let radius = view.transform_vector2(Vec2::splat(radius)).x;

                draw_poly(pos.x, pos.y, sides, radius, rot, color)
            }
            Shape::Triangle(v1, v2, v3) => {
                let transform = *view * Mat3::from_scale_angle_translation(Vec2::ONE, rot, pos);

                let v1 = transform.transform_point2(v1);
                let v2 = transform.transform_point2(v2);
                let v3 = transform.transform_point2(v3);

                draw_triangle(v1, v2, v3, color)
            }
        }
    }
}

#[macroquad::main("Asteroids")]
async fn main() -> Result<()> {
    registry().with(HierarchicalLayer::default()).init();

    let mut world = World::new();

    let dt = 0.02;

    let mut physics_schedule = Schedule::builder()
        .with_system(spawn_asteroids(10))
        .with_system(player_input(dt))
        .with_system(collision_system())
        .with_system(integrate_velocity(dt))
        .with_system(integrate_ang_velocity(dt))
        .with_system(camera_systems(dt))
        .build();

    let mut frame_schedule = Schedule::builder()
        .with_system(draw_shapes())
        .with_system(draw_ui())
        .build();

    let mut acc = 0.0;

    create_player().spawn(&mut world);
    create_camera().spawn(&mut world);

    loop {
        acc += get_frame_time();

        while acc > 0.0 {
            acc -= dt;
            tracing::info!("Executing physics");
            physics_schedule.execute_par(&mut world)?;
        }

        clear_background(BLACK);

        frame_schedule.execute_par(&mut world)?;

        next_frame().await
    }
}

const ASTEROID_SIZE: f32 = 40.0;

fn create_player() -> EntityBuilder {
    Entity::builder()
        .set_default(position())
        .set_default(rotation())
        .set_default(velocity())
        .set_default(angular_velocity())
        .set_default(player())
        .set(mass(), 100.0)
        .set(health(), 100.0)
        .set(
            shape(),
            Shape::Triangle(vec2(-8.0, 16.0), vec2(8.0, 16.0), vec2(0.0, -16.0)),
        )
        .set(radius(), 16.0)
        .set(color(), GREEN)
        .set(
            on_collision(),
            Box::new(|entity, collision| {
                let mut h = entity.get_mut(health()).unwrap();
                *h -= collision.impact;
                tracing::info!("New health: {h:?}");
            }),
        )
        .into()
}

fn create_camera() -> EntityBuilder {
    Entity::builder()
        .set_default(position())
        .set_default(rotation())
        .set(camera(), Mat3::IDENTITY)
        .into()
}

fn camera_systems(dt: f32) -> BoxedSystem {
    System::builder()
        .with(Query::new(position()).with(player()))
        .with(Query::new((position().as_mut(), camera().as_mut())))
        .build(
            move |mut players: QueryData<Component<Vec2>, _>,
                  mut cameras: QueryData<_, _>|
                  -> Result<()> {
                let mut cameras = cameras.borrow();

                let player_pos = *players.borrow().first().ok_or_else(|| eyre!("No player"))?;

                let (camera_pos, camera): (&mut Vec2, &mut Mat3) =
                    cameras.first().ok_or_else(|| eyre!("No camera"))?;

                *camera_pos =
                    camera_pos.lerp(player_pos, (camera_pos.distance(player_pos) * 0.01) * dt);

                let screen_size = vec2(screen_width(), screen_height());

                *camera = Mat3::from_scale_angle_translation(
                    Vec2::ONE,
                    0.0,
                    *camera_pos - screen_size * 0.5,
                )
                .inverse();

                Ok(())
            },
        )
        .boxed()
}

struct Collision {
    a: Entity,
    b: Entity,
    dir: Vec2,
    depth: f32,
    impact: f32,
    system_mass: f32,
}

#[derive(Fetch, Debug, Clone)]
struct CollisionQuery {
    pos: Component<Vec2>,
    vel: Component<Vec2>,
    mass: OptOr<Component<f32>, f32>,
    radius: Component<f32>,
}

impl CollisionQuery {
    pub fn new() -> Self {
        Self {
            pos: position(),
            vel: velocity(),
            mass: mass().opt_or_default(),
            radius: radius(),
        }
    }
}

fn collision_system() -> BoxedSystem {
    System::builder()
        .with_name("collision_system")
        .with(Query::new((entity_ids(), CollisionQuery::new())))
        .with(Query::new((entity_ids(), CollisionQuery::new())))
        .read::<World>()
        .build(
            |mut a: QueryData<(Entities, CollisionQuery)>,
             mut b: QueryData<(Entities, CollisionQuery)>,
             world: Read<World>| {
                let mut a = a.borrow();
                let mut b = b.borrow();

                let mut collisions = Vec::new();

                for (id_a, a) in &mut a {
                    for (id_b, b) in &mut b {
                        if id_a == id_b {
                            continue;
                        }

                        let radii = a.radius + b.radius;

                        let dir = *a.pos - *b.pos;
                        let depth = radii - dir.length();
                        let dir = dir.normalize_or_zero();

                        let impact = (*b.vel - *a.vel).dot(dir);

                        if impact > 0.0 && depth > 0.0 {
                            let system_mass = a.mass + b.mass;

                            collisions.push(Collision {
                                a: id_a,
                                b: id_b,
                                dir,
                                depth,
                                impact,
                                system_mass,
                            });

                            tracing::info!("{id_a} and {id_b} collided");
                        }
                    }
                }

                drop((a, b));

                for collision in collisions {
                    let entity = world.entity(collision.a).unwrap();

                    {
                        let mut pos = entity.get_mut(position()).unwrap();
                        let mut vel = entity.get_mut(velocity()).unwrap();
                        let mass = *entity.get(mass()).unwrap();

                        *vel +=
                            collision.dir * collision.impact * (1.0 - mass / collision.system_mass);
                        *pos +=
                            collision.dir * collision.depth * (1.0 - mass / collision.system_mass);
                    }

                    if let Ok(on_collision) = entity.get(on_collision()) {
                        (on_collision)(entity, collision)
                    };
                }
            },
        )
        .boxed()
}

const SHIP_THRUST: f32 = 100.0;
const SHIP_TURN: f32 = 5.0;

fn player_input(dt: f32) -> BoxedSystem {
    System::builder()
        .with_name("player_input")
        .with(Query::new((velocity().as_mut(), rotation().as_mut())).with(player()))
        .for_each(move |(vel, rot)| {
            let forward = vec2(rot.sin(), -rot.cos());

            let acc = if is_key_down(KeyCode::W) {
                forward * SHIP_THRUST
            } else if is_key_down(KeyCode::S) {
                -forward * SHIP_THRUST
            } else {
                (-*vel * 0.1).clamp_length_max(SHIP_THRUST)
            };

            if is_key_down(KeyCode::A) {
                *rot -= SHIP_TURN * dt;
            }
            if is_key_down(KeyCode::D) {
                *rot += SHIP_TURN * dt;
            }

            tracing::info!("acc: {acc}, vel: {vel} {}", acc.length());
            *vel += acc * dt;
        })
        .boxed()
}

fn spawn_asteroids(max_count: usize) -> BoxedSystem {
    let mut rng = StdRng::from_entropy();
    System::builder()
        .with_name("spawn_asteroids")
        .with(Query::new(position()).with(player()))
        .with(Query::new(asteroid()))
        .write::<CommandBuffer>()
        .build(
            move |mut player: QueryData<Component<Vec2>, _>,
                  mut existing: QueryData<Component<()>>,
                  mut cmd: Write<CommandBuffer>| {
                let player_pos = *player.borrow().first().unwrap();

                let existing = existing.borrow().count();

                let mut builder = Entity::builder();

                (existing..max_count).for_each(|i| {
                    tracing::info!("Spawning asteroid {i}");

                    // Spawn around player
                    let dir = rng.gen_range(0f32..TAU);
                    let pos = player_pos + vec2(dir.cos(), dir.sin()) * rng.gen_range(16.0..512.0);

                    let size = rng.gen_range(0.1..1.0);
                    let radius = size * ASTEROID_SIZE;
                    let health = radius * 100.0;

                    let dir = rng.gen_range(0f32..TAU);
                    let vel = vec2(dir.cos(), dir.sin()) * rng.gen_range(10.0..40.0);

                    builder
                        .set(position(), pos)
                        .set(rotation(), rng.gen())
                        .set_default(asteroid())
                        .set(
                            shape(),
                            Shape::Polygon {
                                radius,
                                sides: rng.gen_range(4..16),
                            },
                        )
                        .set(mass(), radius * radius)
                        .set(self::radius(), radius)
                        .set(self::health(), health)
                        .set(color(), hsl_to_rgb(0.75, 0.5, 0.5))
                        .set(velocity(), vel)
                        .set(angular_velocity(), rng.gen())
                        .spawn_into(&mut cmd);
                })
            },
        )
        .boxed()
}

fn integrate_velocity(dt: f32) -> BoxedSystem {
    System::builder()
        .with_name("integrate_velocity")
        .with(Query::new((position().as_mut(), velocity())))
        .for_each(move |(pos, vel)| {
            *pos += *vel * dt;
        })
        .boxed()
}

fn integrate_ang_velocity(dt: f32) -> BoxedSystem {
    System::builder()
        .with_name("integrate_ang_velocity")
        .with(Query::new((rotation().as_mut(), angular_velocity())))
        .for_each(move |(rot, w)| {
            *rot += *w * dt;
        })
        .boxed()
}

#[derive(Fetch, Debug, Clone)]
struct TransformQuery {
    pos: Component<Vec2>,
    rot: Component<f32>,
}

impl TransformQuery {
    fn new() -> Self {
        Self {
            pos: position(),
            rot: rotation(),
        }
    }
}

fn draw_shapes() -> BoxedSystem {
    System::builder()
        .with_name("draw_asteroids")
        .with(Query::new(camera()))
        .with(Query::new((TransformQuery::new(), shape(), color())))
        .build(
            |mut camera: QueryData<Component<Mat3>>,
             mut q: QueryData<(TransformQuery, Component<Shape>, Component<Color>), _>|
             -> Result<()> {
                let mut camera = camera.borrow();

                let view = camera.first().context("Missing camera")?;

                for (TransformQueryItem { pos, rot }, shape, color) in q.borrow().iter() {
                    shape.draw(view, *pos, *rot, *color);
                }

                Ok(())
            },
        )
        .boxed()
}

fn draw_ui() -> BoxedSystem {
    System::builder()
        .with_name("draw_ui")
        .with(Query::new(health()).with(player()))
        .build(|mut players: QueryData<Component<f32>, _>| {
            let player_health: f32 = *players.borrow().first().unwrap();

            draw_rectangle(10.0, 10.0, 256.0, 16.0, DARKPURPLE);
            draw_rectangle(
                10.0,
                10.0,
                256.0 * (player_health / 100.0).clamp(0.0, 1.0),
                16.0,
                GREEN,
            );
        })
        .boxed()
}
