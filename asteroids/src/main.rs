use color_eyre::{
    eyre::{eyre, ContextCompat},
    Result,
};
use std::f32::consts::TAU;
use tracing_subscriber::{prelude::*, registry};

use flax::*;
use glam::*;
use macroquad::{
    color::hsl_to_rgb,
    prelude::{is_key_down, Color, KeyCode, BLACK, BLUE, GRAY, GREEN, ORANGE},
    shapes::{draw_circle, draw_poly, draw_triangle},
    text::draw_text,
    time::get_frame_time,
    window::{clear_background, next_frame, screen_height, screen_width},
};
use rand::{rngs::StdRng, thread_rng, Rng, SeedableRng};
use tracing_tree::HierarchicalLayer;

component! {
    position: Vec2 => [ Debug ],
    rotation: f32 => [ Debug ],

    asteroid: () => [ Debug ],
    player: () => [ Debug ],

    /// The amount of material collected from asteroids
    material: f32 => [ Debug ],

    camera: Mat3 => [ Debug ],
    health: f32 => [ Debug ],
    color: Color => [ Debug ],
    mass: f32 => [ Debug ],
    difficulty: f32 => [Debug],

    velocity: Vec2=> [ Debug ],
    angular_velocity: f32 => [ Debug ],

    shape: Shape => [ Debug ],
    radius: f32 => [ Debug ],

    particle_size: f32,
    particle_lifetime: f32,

    on_collision: Box<dyn Fn(&World, Collision) + Send + Sync>,

    lifetime: f32 => [ Debug ],

}

/// Macroquad has unsound race conditions, as such, use a mock shared
/// context
#[derive(Hash, Debug, Clone)]
struct GraphicsContext;

#[derive(Debug, Clone)]
enum Shape {
    Polygon { radius: f32, sides: u8 },
    Circle { radius: f32 },
    Triangle(Vec2, Vec2, Vec2),
}

impl Shape {
    pub fn draw(&self, view: &Mat3, pos: Vec2, rot: f32, color: Color) {
        match *self {
            Shape::Circle { radius } => {
                let pos = view.transform_point2(pos);
                let radius = view.transform_vector2(Vec2::splat(radius)).x;
                draw_circle(pos.x, pos.y, radius, color)
            }
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

    let (player_dead_tx, player_dead_rx) = flume::unbounded();

    world.on_removed(player(), player_dead_tx);

    let mut physics_schedule = Schedule::builder()
        .with_system(spawn_asteroids(16))
        .with_system(player_system(dt))
        .with_system(camera_systems(dt))
        .with_system(lifetime_system(dt))
        .with_system(despawn_out_of_bounds())
        .with_system(particle_system())
        .with_system(collision_system())
        .with_system(integrate_velocity(dt))
        .with_system(integrate_ang_velocity(dt))
        .with_system(despawn_dead())
        .build();

    let mut frame_schedule = Schedule::builder()
        .with_system(draw_shapes())
        .with_system(draw_ui())
        .build();

    let mut acc = 0.0;

    create_player().spawn(&mut world);
    create_camera().spawn(&mut world);

    loop {
        if player_dead_rx.try_recv().is_ok() {
            create_player().spawn(&mut world);
        }

        acc += get_frame_time();

        while acc > 0.0 {
            acc -= dt;
            tracing::info!("Executing physics");
            physics_schedule.execute_par(&mut world)?;
            tracing::info!("physics_schedule: {:#?}", physics_schedule.batch_info());
        }

        clear_background(BLACK);

        frame_schedule.execute_par(&mut world)?;
        tracing::info!("frame_schedule: {:#?}", frame_schedule.batch_info());

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
        .set_default(material())
        .set(color(), GREEN)
        .set(
            on_collision(),
            Box::new(|world, collision| {
                let mut h = world.get_mut(collision.a, health()).unwrap();
                if collision.impact > 10.0 {
                    *h -= 20.0;
                }
            }),
        )
        .set(difficulty(), 1.0)
        .into()
}

fn create_camera() -> EntityBuilder {
    Entity::builder()
        .set_default(position())
        .set_default(rotation())
        .set(camera(), Mat3::IDENTITY)
        .into()
}

const BULLET_DAMAGE: f32 = 20.0;
const BULLET_SPEED: f32 = 200.0;

fn create_bullet(player: Entity) -> EntityBuilder {
    Entity::builder()
        .set_default(velocity())
        .set_default(position())
        .set_default(rotation())
        .set(mass(), 10.0)
        .set(health(), 100.0)
        .set(shape(), Shape::Circle { radius: 4.0 })
        .set(radius(), 4.0)
        .set(lifetime(), 5.0)
        .set(color(), BLUE)
        .set(
            on_collision(),
            Box::new(move |world, coll| {
                *world.get_mut(coll.a, health()).unwrap() = 0.0;

                if let Ok(mut health) = world.get_mut(coll.b, health()) {
                    if *health <= 0.0 {
                        return;
                    }

                    *health -= BULLET_DAMAGE;

                    if *health <= 0.0 {
                        if let Ok(mut material) = world.get_mut(player, material()) {
                            *material += world
                                .get_mut(coll.b, self::material())
                                .as_deref()
                                .copied()
                                .unwrap_or_default()
                        }
                    }
                }
            }),
        )
        .into()
}

fn create_particle(size: f32, lifetime: f32, color: Color) -> EntityBuilder {
    Entity::builder()
        .set_default(rotation())
        .set_default(position())
        .set(shape(), Shape::Circle { radius: size })
        .set(particle_size(), size)
        .set(self::lifetime(), lifetime)
        .set(particle_lifetime(), lifetime)
        .set(self::color(), color)
        .into()
}

fn create_explosion(
    count: usize,
    pos: Vec2,
    speed: f32,
    size: f32,
    lifetime: f32,
    color: Color,
) -> impl Iterator<Item = EntityBuilder> {
    let mut rng = thread_rng();
    (0..count).map(move |_| {
        let dir = rng.gen_range(0.0..TAU);
        let speed = rng.gen_range(speed * 0.5..speed);
        create_particle(size, lifetime, color)
            .set(velocity(), speed * vec2(dir.cos(), dir.sin()))
            .set(position(), pos)
            .into()
    })
}

fn particle_system() -> BoxedSystem {
    System::builder()
        .with_name("particle_system")
        .with(Query::new((
            lifetime(),
            particle_size(),
            particle_lifetime(),
            shape().as_mut(),
        )))
        .for_each(|(lifetime, size, max_lifetime, shape)| {
            *shape = Shape::Circle {
                radius: lifetime / max_lifetime * size,
            };
        })
        .boxed()
}

fn camera_systems(dt: f32) -> BoxedSystem {
    System::builder()
        .with(Query::new(position()).with(player()))
        .with(Query::new((position().as_mut(), camera().as_mut())))
        .build(
            move |mut players: QueryBorrow<Component<Vec2>, _>,
                  mut cameras: QueryBorrow<_, _>|
                  -> Result<()> {
                let player_pos = players.first().copied().unwrap_or_default();

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
    point: Vec2,
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

fn lifetime_system(dt: f32) -> BoxedSystem {
    System::builder()
        .with_name("lifetime_system")
        .with(Query::new((entity_ids(), lifetime().as_mut())))
        .write::<CommandBuffer>()
        .build(
            move |mut q: QueryBorrow<(EntityIds, Mutable<f32>)>, cmd: &mut CommandBuffer| {
                for (id, lf) in &mut q {
                    if *lf <= 0.0 {
                        cmd.set(id, health(), 0.0);
                    }
                    *lf -= dt;
                }
            },
        )
        .boxed()
}

fn collision_system() -> BoxedSystem {
    System::builder()
        .with_name("collision_system")
        .with(Query::new((entity_ids(), CollisionQuery::new())))
        .with(Query::new((entity_ids(), CollisionQuery::new())))
        .read::<World>()
        .write::<CommandBuffer>()
        .build(
            |mut a: QueryBorrow<(EntityIds, CollisionQuery)>,
             mut b: QueryBorrow<(EntityIds, CollisionQuery)>,
             world: &World,
             cmd: &mut CommandBuffer| {
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
                                point: *a.pos + (*a.radius) * -dir,
                                dir,
                                depth,
                                impact,
                                system_mass,
                            });
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

                    create_explosion(8, collision.point, collision.impact, 4.0, 1.0, GRAY)
                        .for_each(|v| {
                            cmd.spawn(v);
                        });

                    if let Ok(on_collision) = entity.get(on_collision()) {
                        (on_collision)(world, collision)
                    };
                }
            },
        )
        .boxed()
}

const SHIP_THRUST: f32 = 150.0;
const SHIP_TURN: f32 = 2.0;
const WEAPON_COOLDOWN: f32 = 0.2;
const PLUME_COOLDOWN: f32 = 0.02;

fn player_system(dt: f32) -> BoxedSystem {
    let mut current_weapon_cooldown = 0.0;
    let mut current_plume_cooldown = 0.0;

    System::builder()
        .with_name("player_system")
        .with(
            Query::new((
                entity_ids(),
                position(),
                material(),
                velocity().as_mut(),
                rotation().as_mut(),
                difficulty().as_mut(),
            ))
            .with(player()),
        )
        .write::<CommandBuffer>()
        .build(
            move |mut q: QueryBorrow<
                (
                    EntityIds,
                    Component<Vec2>,
                    Component<f32>,
                    Mutable<Vec2>,
                    Mutable<f32>,
                    Mutable<f32>,
                ),
                _,
            >,
                  cmd: &mut CommandBuffer| {
                current_weapon_cooldown -= dt;
                current_plume_cooldown -= dt;

                for (player, pos, material, vel, rot, current_difficulty) in &mut q {
                    *current_difficulty = (*material * 0.001).max(1.0);

                    let forward = vec2(rot.sin(), -rot.cos());

                    let acc = if is_key_down(KeyCode::W) {
                        forward * SHIP_THRUST
                    } else if is_key_down(KeyCode::S) {
                        -forward * SHIP_THRUST
                    } else {
                        Vec2::ZERO
                    };

                    if acc.length() > 0.0 && current_plume_cooldown <= 0.0 {
                        current_plume_cooldown = PLUME_COOLDOWN;
                        create_particle(8.0, 0.5, ORANGE)
                            .set(position(), *pos - 30.0 * forward)
                            .set(velocity(), *vel + -acc)
                            .spawn_into(cmd)
                    }

                    if is_key_down(KeyCode::A) {
                        *rot -= SHIP_TURN * dt;
                    }
                    if is_key_down(KeyCode::D) {
                        *rot += SHIP_TURN * dt;
                    }

                    if is_key_down(KeyCode::Space) && current_weapon_cooldown <= 0.0 {
                        current_weapon_cooldown = WEAPON_COOLDOWN;
                        create_bullet(player)
                            .set(velocity(), *vel + BULLET_SPEED * forward)
                            .set(position(), *pos + 30.0 * forward)
                            .spawn_into(cmd)
                    }

                    *vel += acc * dt;
                }
            },
        )
        .boxed()
}
fn despawn_out_of_bounds() -> BoxedSystem {
    System::builder()
        .with_name("despawn_out_of_bounds")
        .with(Query::new(position()).with(player()))
        .with(Query::new((position(), health().as_mut())).without(player()))
        .build(
            |mut player: QueryBorrow<Component<Vec2>, _>,
             mut asteroids: QueryBorrow<(Component<Vec2>, Mutable<f32>), _>| {
                let player_pos = *player.first().unwrap();

                for (asteroid, health) in &mut asteroids {
                    if player_pos.distance(*asteroid) > 2000.0 {
                        *health = 0.0;
                    }
                }
            },
        )
        .boxed()
}

fn despawn_dead() -> BoxedSystem {
    System::builder()
        .with_name("despawn_dead")
        .with(Query::new(entity_ids()).filter(health().lte(0.0)))
        .write::<CommandBuffer>()
        .build(
            |mut q: QueryBorrow<EntityIds, _>, cmd: &mut CommandBuffer| {
                for id in &mut q {
                    cmd.despawn(id);
                }
            },
        )
        .boxed()
}

fn spawn_asteroids(max_count: usize) -> BoxedSystem {
    let mut rng = StdRng::from_entropy();

    System::builder()
        .with_name("spawn_asteroids")
        .with(Query::new((position(), difficulty())).with(player()))
        .with(Query::new(asteroid()))
        .write::<CommandBuffer>()
        .build(
            move |mut players: QueryBorrow<(Component<Vec2>, Component<f32>), _>,
                  mut existing: QueryBorrow<Component<()>>,
                  cmd: &mut CommandBuffer| {
                let (player_pos, difficulty) = match players.first() {
                    Some(v) => v,
                    None => return,
                };

                let existing = existing.count();

                let mut builder = Entity::builder();

                (existing..max_count).for_each(|_| {
                    // Spawn around player
                    let dir = rng.gen_range(0f32..TAU);
                    let pos =
                        *player_pos + vec2(dir.cos(), dir.sin()) * rng.gen_range(128.0..1024.0);

                    let size = rng.gen_range(0.2..1.0);
                    let radius = size * ASTEROID_SIZE;
                    let health = size * 100.0;

                    let dir = rng.gen_range(0f32..TAU);
                    let vel =
                        vec2(dir.cos(), dir.sin()) * rng.gen_range(30.0..80.0) * difficulty.sqrt();

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
                        .set(self::material(), radius * radius)
                        .set(self::radius(), radius)
                        .set(self::health(), health)
                        .set(color(), hsl_to_rgb(0.75, 0.5, 0.5))
                        .set(velocity(), vel)
                        .set(angular_velocity(), rng.gen_range(-2.0..2.0))
                        .spawn_into(cmd);
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
        .with_resource(GraphicsContext)
        .with(Query::new(camera()))
        .with(Query::new((TransformQuery::new(), shape(), color())))
        .build(
            |_ctx: &mut GraphicsContext,
             mut camera: QueryBorrow<Component<Mat3>>,
             mut q: QueryBorrow<(TransformQuery, Component<Shape>, Component<Color>), _>|
             -> Result<()> {
                let view = camera.first().context("Missing camera")?;

                for (TransformQueryItem { pos, rot }, shape, color) in &mut q {
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
        .with_resource(GraphicsContext)
        .with(Query::new((material(), health(), difficulty())).with(player()))
        .with(Query::new(()))
        .build(
            |_ctx: &mut GraphicsContext,
             mut players: QueryBorrow<(Component<f32>, Component<f32>, Component<f32>), _>,
             mut all: QueryBorrow<(), _>| {
                let count = all.count();

                let result = players.first();

                if let Some((material, health, _difficulty)) = result {
                    draw_text(
                        &format!("Hull: {}%", health.round()),
                        10.0,
                        32.0,
                        32.0,
                        Color::from_vec(
                            vec4(1.0, 0.0, 0.0, 1.0).lerp(vec4(0.0, 1.0, 0.0, 1.0), health / 100.0),
                        ),
                    );

                    draw_text(
                        &format!("Material: {}kg", material.round()),
                        10.0,
                        64.0,
                        32.0,
                        BLUE,
                    );

                    draw_text(&format!("Entities: {count}"), 10.0, 96.0, 16.0, GRAY);

                    // draw_rectangle(10.0, 10.0, 256.0, 16.0, DARKPURPLE);
                    // draw_rectangle(
                    //     10.0,
                    //     10.0,
                    //     256.0 * (player_health / 100.0).clamp(0.0, 1.0),
                    //     16.0,
                    //     GREEN,
                    // );
                }
            },
        )
        .boxed()
}
