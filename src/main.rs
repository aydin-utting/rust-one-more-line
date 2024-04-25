use ggez::glam::Vec2;
use ggez::input::keyboard::{KeyCode, KeyInput};
use ggez::*;
use oorandom::Rand32;
use std::env;
use std::f32::consts::PI;
use std::path;
use std::time::{SystemTime, UNIX_EPOCH};

const SCREEN_HEIGHT: f32 = 848.0;
const SCREEN_WIDTH: f32 = 480.0;
const AREA_HEIGHT: f32 = 5.0;
const AREA_WIDTH: f32 = (SCREEN_WIDTH / SCREEN_HEIGHT) * AREA_HEIGHT;
const MAX_TIME_OUTSIDE: f32 = 0.5;
const RATIO: f32 = SCREEN_HEIGHT / AREA_HEIGHT;

const COLORS: [graphics::Color; 6] = [
    graphics::Color::WHITE,
    graphics::Color::MAGENTA,
    graphics::Color::CYAN,
    graphics::Color::GREEN,
    graphics::Color::RED,
    graphics::Color::YELLOW,
];

#[derive(Debug)]
enum Attach {
    SUCCESS(Node, bool),
    TARGET(Node, bool),
    None,
}

/// Translates the world coordinate system, which
/// has Y pointing up and the origin at the bottom middle,
/// to the screen coordinate system, which has Y
/// pointing downward and the origin at the top-left,
fn world_to_screen_coords(
    screen_width: f32,
    screen_height: f32,
    point: Vec2,
    origin: Vec2,
) -> Vec2 {
    let new_point = point - origin;
    let screen_point_y = screen_height / AREA_HEIGHT * new_point.y;
    let screen_point_x = screen_width / AREA_WIDTH * new_point.x;
    let x = screen_point_x + screen_width / 2.0;
    let y = screen_height - screen_point_y;
    Vec2::new(x, y)
}

#[derive(Debug)]
struct Assets {
    player_image: graphics::Image,
    hit_sound: audio::Source,
}

impl Assets {
    fn new(ctx: &mut Context) -> GameResult<Assets> {
        let player_image =
            graphics::Image::from_path(ctx, "/player.png").expect("Can't load player image");
        let hit_sound = audio::Source::new(ctx, "/boom.ogg").expect("Can't load hit sound");
        Ok(Assets {
            player_image,
            hit_sound,
        })
    }
}
#[derive(Debug)]
struct Player {
    pos: Vec2,
    speed: f32,
    facing: f32,
    bbox: f32,
    time_disconnected: f32,
}

impl Player {
    fn new() -> GameResult<Player> {
        Ok(Player {
            pos: Vec2::ZERO,
            speed: 4.0,
            facing: 0.0,
            bbox: 0.05,
            time_disconnected: 0.0,
        })
    }

    fn draw(
        self: &Player,
        assets: &mut Assets,
        canvas: &mut graphics::Canvas,
        origin: Vec2,
        screen_w: f32,
        screen_h: f32,
    ) {
        let image = &assets.player_image;

        let pos = world_to_screen_coords(screen_w, screen_h, self.pos, origin);
        let drawparams = graphics::DrawParam::new()
            .dest(pos)
            .rotation(self.facing)
            .offset(Vec2::new(0.5, 0.5));
        canvas.draw(image, drawparams);
    }

    fn orbit(self: &mut Player, node: &Node, dt: f32, is_clockwise: bool) {
        let mult = if is_clockwise { -1.0 } else { 1.0 };
        let fac = if is_clockwise { Vec2::NEG_X } else { Vec2::X };
        let radius = node.pos.distance(self.pos);
        let circ = 2.0 * PI * radius;
        let period = circ / self.speed;
        let delta = self.pos - node.pos;
        self.pos = Vec2::from_angle(mult * 2.0 * PI * dt / period).rotate(delta) + node.pos;
        self.facing = delta.angle_between(fac);
    }
}

#[derive(Debug, Copy, Clone)]
struct Node {
    pos: Vec2,
    radius: f32,
    color: graphics::Color,
}

impl Node {
    fn add_mesh(
        self: &Node,
        mb: &mut graphics::MeshBuilder,
        origin: Vec2,
        screen_w: f32,
        screen_h: f32,
    ) {
        let pos = world_to_screen_coords(screen_w, screen_h, self.pos, origin);
        mb.circle(
            graphics::DrawMode::fill(),
            pos,
            self.radius * RATIO,
            1.0,
            self.color,
        )
        .expect("Something went wrong rendering a node");
    }
}

#[derive(Debug)]
struct State {
    assets: Assets,
    player: Player,
    nodes: Vec<Node>,
    attached_node: Attach,
    screen_width: f32,
    screen_height: f32,
    prev_points: Vec<Vec2>,
}

fn make_nodes(begin: u32, end: u32, mut rng: Rand32) -> Vec<Node> {
    (begin..=end)
        .map(|i| {
            let y = rng.rand_float() - 0.5 + 1.5 * (i as f32);
            let x = AREA_WIDTH * (rng.rand_float() - 0.5);
            Node {
                pos: Vec2 { x, y },
                radius: (rng.rand_float() * (0.25 - 0.05)) + 0.05,
                color: COLORS[(i % 6) as usize],
            }
        })
        .collect()
}

fn get_cross_point(player: &Player, node: &Node) -> Vec2 {
    let vel = Vec2::from_angle(PI / 2.0 - player.facing).normalize();
    let player_to_node = node.pos - player.pos;
    let angle = vel.angle_between(player_to_node);
    let dist = angle.cos() * player_to_node.length() * vel;
    let cross_point = player.pos + dist;
    cross_point
}
fn get_is_behind(player: &Player, node: &Node) -> bool {
    let vel = Vec2::from_angle(PI / 2.0 - player.facing).normalize();
    let player_to_node = node.pos - player.pos;
    let angle = vel.angle_between(player_to_node);
    return angle.cos() < 0.0;
}

fn filter_deadly_nodes(player: &Player, node: &Node) -> bool {
    let cross_point = get_cross_point(player, node);
    let is_behind = get_is_behind(player, node);
    let is_outside = cross_point.x.abs() > AREA_WIDTH / 2.0;
    let is_hitting = node.pos.distance(cross_point) < player.bbox + node.radius;
    let is_far_away = player.pos.distance(node.pos) > 2.0;
    !(is_outside || is_hitting || is_far_away || is_behind)
}

fn filter_hitting_nodes(player: &Player, node: &Node) -> bool {
    let cross_point = get_cross_point(player, node);
    let is_hitting = node.pos.distance(cross_point) < player.bbox + node.radius;
    !is_hitting
}

fn get_is_clockwise(player: &Player, node: &Node) -> bool {
    let delta = player.pos - node.pos;
    let angle = delta.angle_between(Vec2::from_angle(PI / 2.0 - player.facing));
    let is_clockwise = angle < 0.0;
    is_clockwise
}

impl State {
    fn new(ctx: &mut Context) -> GameResult<State> {
        let start = SystemTime::now();
        let since_the_epoch = start
            .duration_since(UNIX_EPOCH)
            .expect("Time went backwards");
        let (width, height) = ctx.gfx.drawable_size();
        let rng = Rand32::new(since_the_epoch.as_secs());
        let player = Player::new()?;
        let nodes = make_nodes(0, 100, rng);
        let assets = Assets::new(ctx)?;

        Ok(State {
            player,
            nodes,
            assets,
            attached_node: Attach::None,
            screen_height: height,
            screen_width: width,
            prev_points: Vec::new(),
        })
    }

    fn handle_collision(self: &Self) -> bool {
        let is_hitting_side = match self.attached_node {
            Attach::SUCCESS(_, _) => false,
            _ => {
                (self.player.pos.x.abs() - (AREA_WIDTH / 2.0)).abs() < self.player.bbox
                    && self.player.time_disconnected > 0.1
            }
        };
        let is_hitting_node = self
            .nodes
            .iter()
            .any(|n| self.player.pos.distance(n.pos) < self.player.bbox + n.radius);

        let is_outside_too_long = self.player.pos.x.abs() > AREA_WIDTH / 2.0
            && self.player.time_disconnected > MAX_TIME_OUTSIDE;

        return is_hitting_side || is_hitting_node || is_outside_too_long;
    }

    fn handle_button_press(self: &mut Self) {
        match self.attached_node {
            Attach::None => {
                match self
                    .nodes
                    .iter()
                    .filter(|n| filter_deadly_nodes(&self.player, n))
                    .min_by(|a, b| {
                        let axp = get_cross_point(&self.player, a);
                        let bxp = get_cross_point(&self.player, b);
                        match axp
                            .distance_squared(self.player.pos)
                            .partial_cmp(&bxp.distance_squared(self.player.pos))
                        {
                            Some(ordering) => ordering,
                            None => std::cmp::Ordering::Greater,
                        }
                    }) {
                    Some(n) => {
                        let delta = n.pos - self.player.pos;
                        let angle =
                            Vec2::from_angle(PI / 2.0 - self.player.facing).angle_between(delta);
                        if angle.cos().abs() < 0.1 {
                            self.attached_node =
                                Attach::SUCCESS(*n, get_is_clockwise(&self.player, n));
                            self.player.time_disconnected = 0.0;
                        } else {
                            self.attached_node =
                                Attach::TARGET(*n, get_is_clockwise(&self.player, n));
                        }
                    }
                    None => {
                        match self
                            .nodes
                            .iter()
                            .filter(|n| filter_hitting_nodes(&self.player, n))
                            .min_by(|a, b| {
                                a.pos
                                    .distance_squared(self.player.pos)
                                    .partial_cmp(&b.pos.distance_squared(self.player.pos))
                                    .unwrap()
                            }) {
                            Some(n) => {
                                self.attached_node =
                                    Attach::SUCCESS(*n, get_is_clockwise(&self.player, n));
                                self.player.time_disconnected = 0.0;
                            }
                            None => {
                                self.attached_node = Attach::None;
                            }
                        }
                    }
                };
            }
            _ => {}
        };
    }

    fn reset(self: &mut Self) {
        self.player.pos = Vec2::new(0.0, 0.0);
        self.player.facing = 0.0;
        self.player.time_disconnected = 0.0;
        self.attached_node = Attach::None;
        self.prev_points = vec![];
    }
}

impl ggez::event::EventHandler<GameError> for State {
    fn update(&mut self, ctx: &mut Context) -> Result<(), GameError> {
        let dt = ctx.time.delta().as_secs_f32();
        match self.attached_node {
            Attach::SUCCESS(node, is_clockwise) => {
                self.player.orbit(&node, dt, is_clockwise);
            }
            Attach::TARGET(node, is_clockwise) => {
                let delta = node.pos - self.player.pos;
                let angle = Vec2::from_angle(PI / 2.0 - self.player.facing).angle_between(delta);
                if angle.cos().abs() < 0.1 {
                    self.attached_node = Attach::SUCCESS(node, is_clockwise);
                    self.player.time_disconnected = 0.0;
                    self.player.orbit(&node, dt, is_clockwise);
                } else {
                    self.player.pos +=
                        self.player.speed * dt * Vec2::from_angle(PI / 2.0 - self.player.facing);
                }
            }
            Attach::None => {
                self.player.pos +=
                    self.player.speed * dt * Vec2::from_angle(PI / 2.0 - self.player.facing);
            }
        };
        self.prev_points.push(self.player.pos);
        if self.prev_points.len() > 100 {
            self.prev_points = self.prev_points[self.prev_points.len() - 100..].to_vec()
        };
        match self.attached_node {
            Attach::SUCCESS(_, _) => (),
            _ => {
                self.player.time_disconnected += dt;
            }
        };
        if self.handle_collision() {
            self.reset();
        }
        Ok(())
    }

    fn draw(&mut self, ctx: &mut Context) -> Result<(), GameError> {
        let coord_origin = Vec2::new(self.player.pos.x / 2.0, self.player.pos.y - 1.0);
        let wtsc = |pos: Vec2| {
            world_to_screen_coords(self.screen_width, self.screen_height, pos, coord_origin)
        };
        let mut canvas = graphics::Canvas::from_frame(ctx, graphics::Color::BLACK);
        self.player.draw(
            &mut self.assets,
            &mut canvas,
            coord_origin,
            self.screen_width,
            self.screen_height,
        );
        let mb = &mut graphics::MeshBuilder::new();
        let border_line_color = match self.attached_node {
            Attach::SUCCESS(_, _) => graphics::Color::from_rgb(100, 100, 100),
            _ => graphics::Color::RED,
        };

        mb.line(
            &vec![
                wtsc(Vec2::new(
                    -AREA_WIDTH / 2.0,
                    self.player.pos.y - AREA_HEIGHT,
                )),
                wtsc(Vec2::new(
                    -AREA_WIDTH / 2.0,
                    self.player.pos.y + AREA_HEIGHT,
                )),
            ],
            5.0,
            border_line_color,
        )
        .unwrap();

        mb.line(
            &vec![
                wtsc(Vec2::new(AREA_WIDTH / 2.0, self.player.pos.y - AREA_HEIGHT)),
                wtsc(Vec2::new(AREA_WIDTH / 2.0, self.player.pos.y + AREA_HEIGHT)),
            ],
            5.0,
            border_line_color,
        )
        .unwrap();

        for n in &self.nodes {
            n.add_mesh(mb, coord_origin, self.screen_width, self.screen_height);
            // // Uncomment this block to show valid node lines
            // if filter_deadly_nodes(&self.player, n) {
            //     mb.line(
            //         &[
            //             wtsc(self.player.pos),
            //             wtsc(get_cross_point(&self.player, n)),
            //             wtsc(n.pos),
            //         ],
            //         1.0,
            //         n.color,
            //     )
            //     .unwrap();
            // }
        }

        let add_line = |mb: &mut graphics::MeshBuilder, node: &Node| {
            let node_pos = wtsc(node.pos);
            let player_pos = wtsc(self.player.pos);
            let radius = node_pos.distance(player_pos);
            mb.circle(
                graphics::DrawMode::Stroke(graphics::StrokeOptions::DEFAULT),
                node_pos,
                radius,
                1.0,
                graphics::Color::WHITE,
            )
            .unwrap();
            mb.line(&[node_pos, player_pos], 5.0, graphics::Color::WHITE)
                .unwrap();
        };

        let add_target_line = |mb: &mut graphics::MeshBuilder, node: &Node| {
            let node_pos = wtsc(node.pos);
            let player_pos = wtsc(self.player.pos);
            mb.line(&[node_pos, player_pos], 5.0, node.color).unwrap();
        };

        match self.attached_node {
            Attach::SUCCESS(node, _) => {
                add_line(mb, &node);
            }
            Attach::TARGET(node, _) => {
                add_target_line(mb, &node);
            }
            Attach::None => {}
        };
        if self.prev_points.len() > 1 {
            let prev_points: Vec<Vec2> = self
                .prev_points
                .iter()
                .map(|p| {
                    world_to_screen_coords(self.screen_width, self.screen_height, *p, coord_origin)
                })
                .collect();
            // Draw THE line!
            mb.line(&prev_points, 5.0, graphics::Color::WHITE).unwrap();
        }
        let mesh = graphics::Mesh::from_data(ctx, mb.build());
        canvas.draw(&mesh, graphics::DrawParam::new());

        let score_dest = Vec2::new(10.0, 10.0);
        let score_str = format!("Level: {}", self.player.pos.y.round());

        canvas.draw(
            &graphics::Text::new(score_str),
            graphics::DrawParam::from(score_dest).color(ggez::graphics::Color::WHITE),
        );

        canvas.finish(ctx)?;
        Ok(())
    }
    // Handle key events.  These just map keyboard events
    // and alter our input state appropriately.
    fn key_down_event(
        &mut self,
        ctx: &mut Context,
        input: KeyInput,
        _repeated: bool,
    ) -> GameResult {
        match input.keycode {
            Some(KeyCode::Space) => {
                self.handle_button_press();
            }
            Some(KeyCode::Escape) => ctx.request_quit(),
            _ => (), // Do nothing
        }
        Ok(())
    }

    fn key_up_event(&mut self, _ctx: &mut Context, input: KeyInput) -> GameResult {
        match input.keycode {
            Some(KeyCode::Space) => {
                self.attached_node = Attach::None;
            }
            _ => (), // Do nothing
        }
        Ok(())
    }
}

fn main() {
    // We add the CARGO_MANIFEST_DIR/resources to the resource paths
    // so that ggez will look in our cargo project directory for files.
    let resource_dir = if let Ok(manifest_dir) = env::var("CARGO_MANIFEST_DIR") {
        let mut path = path::PathBuf::from(manifest_dir);
        path.push("resources");
        path
    } else {
        path::PathBuf::from("./resources")
    };
    // let resource_dir = path::PathBuf::from("./resources");

    let cb = ContextBuilder::new("hello_ggez", "aydin")
        .window_mode(conf::WindowMode::default().dimensions(SCREEN_WIDTH, SCREEN_HEIGHT))
        .add_resource_path(resource_dir);
    let (mut ctx, event_loop) = cb.build().unwrap();
    let state = State::new(&mut ctx).unwrap();
    event::run(ctx, event_loop, state);
}
