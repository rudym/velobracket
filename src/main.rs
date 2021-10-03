mod lib;

bracket_terminal::add_wasm_support!();
use bracket_terminal::prelude::*;

use crate::lib::read_arguments;

use crate::comp::{humanoid, Body};
use specs::prelude::*;
use std::{
    io,
    io::{stdin, stdout, Write},
    process,
    sync::{mpsc, Arc},
    thread,
    time::Duration,
};
use tokio::runtime::Runtime;
use vek::*;

use veloren_client::{
    addr::ConnectionArgs, Client, Error, Event, Join, Marker, MarkerAllocator, WorldExt,
};

use veloren_common::{
    clock::Clock,
    comp,
    comp::fluid_dynamics::LiquidKind,
    comp::inventory::slot::Slot,
    comp::InputKind,
    terrain::{
        structure::{self, StructureBlock},
        Block, BlockKind, SpriteKind,
    },
    uid::UidAllocator,
    vol::ReadVol,
};

struct State {
    ecs: World,
    zoom_level: f32,
    chat_log: Vec<String>,
    chat_input: String,
    chat_input_enabled: bool,
    inv_toggle: bool,
    is_jump_active: bool,
    is_secondary_active: bool,
    is_primary_active: bool,
    is_glide_active: bool,
    invpos: u32,
    arrowed1: Option<Slot>,
    arrowed2: Option<Slot>,
    arrowed: Option<Slot>,
    use_slotid: Option<Slot>,
    arrowedpos: u32,
    swap: bool,
    use_item: bool,
}

impl GameState for State {
    fn tick(&mut self, ctx: &mut BTerm) {
        ctx.cls();

        let mut clock = self.ecs.fetch_mut::<Clock>();
        let mut client = self.ecs.fetch_mut::<Client>();

        {
            // Get Health and Energy
            let (current_health, max_health) = client
                .current::<comp::Health>()
                .map_or((0.0, 0.0), |health| (health.current(), health.maximum()));
            let (current_energy, max_energy) = client
                .current::<comp::Energy>()
                .map_or((0.0, 0.0), |energy| (energy.current(), energy.maximum()));

            // Invite Logic
            let (inviter_uid, invite_kind) =
                if let Some((inviter_uid, _, _, invite_kind)) = client.invite() {
                    (Some(inviter_uid), Some(invite_kind))
                } else {
                    (None, None)
                };

            //Get entity username from UID
            let inviter_username = if let Some(uid) = inviter_uid {
                if let Some(entity) = client
                    .state()
                    .ecs()
                    .read_resource::<UidAllocator>()
                    .retrieve_entity_internal(uid.id())
                {
                    if let Some(player) = client.state().read_storage::<comp::Player>().get(entity)
                    {
                        player.alias.clone()
                    } else {
                        "".to_string()
                    }
                } else {
                    "".to_string()
                }
            } else {
                "".to_string()
            };

            //Get player pos
            let screen_size = Vec2::new(80, 50);
            let player_pos = client
                .state()
                .read_storage::<comp::Pos>()
                .get(client.entity())
                .map(|pos| pos.0)
                .unwrap_or(Vec3::zero());
            let to_screen_pos = |pos: Vec2<f32>, zoom_level: f32| {
                ((pos - Vec2::from(player_pos)) * Vec2::new(1.0, -1.0) / zoom_level
                    + screen_size.map(|e| e as f32) / 2.0)
                    .map(|e| e as i32)
            };

            let mut inputs = comp::ControllerInputs::default();

            // Handle inputs
            match ctx.key {
                None => {} // Nothing happened
                Some(key) => {
                    // A key is pressed or held
                    match key {
                        // Chat
                        VirtualKeyCode::C if self.chat_input_enabled => match key {
                            VirtualKeyCode::Return => {
                                if self.chat_input.is_empty() {
                                } else {
                                    if self.chat_input.clone().starts_with('/') {
                                        let mut argv = self.chat_input.clone();
                                        client.send_command(
                                            argv.split_whitespace().next().unwrap().to_owned(),
                                            argv.split_whitespace().map(|s| s.to_owned()).collect(),
                                        );
                                    } else {
                                        client.send_chat(self.chat_input.clone())
                                    }
                                    self.chat_input = String::new();
                                }
                                self.chat_input_enabled = false;
                            }
                            VirtualKeyCode::Back => {
                                self.chat_input.pop();
                            }
                            _key => self
                                .chat_input
                                .push(format!("{}", key as i32).pop().unwrap()),
                        },

                        // Numpad
                        VirtualKeyCode::Numpad8 | VirtualKeyCode::W => inputs.move_dir.y += 1.0,
                        VirtualKeyCode::Numpad4 | VirtualKeyCode::A => inputs.move_dir.x -= 1.0,
                        VirtualKeyCode::Numpad2 | VirtualKeyCode::Numpad5 | VirtualKeyCode::S => {
                            inputs.move_dir.y -= 1.0
                        }
                        VirtualKeyCode::Numpad6 | VirtualKeyCode::D => inputs.move_dir.x += 1.0,
                        VirtualKeyCode::Numpad7 => {
                            inputs.move_dir.x -= 1.0;
                            inputs.move_dir.y += 1.0;
                        }
                        VirtualKeyCode::Numpad9 => {
                            inputs.move_dir.x += 1.0;
                            inputs.move_dir.y += 1.0;
                        }
                        VirtualKeyCode::Numpad1 => {
                            inputs.move_dir.x -= 1.0;
                            inputs.move_dir.y -= 1.0;
                        }
                        VirtualKeyCode::Numpad3 => {
                            inputs.move_dir.x += 1.0;
                            inputs.move_dir.y -= 1.0;
                        }

                        VirtualKeyCode::Down => self.invpos += 1,
                        VirtualKeyCode::Up => self.invpos -= 1,
                        VirtualKeyCode::Right => match self.arrowedpos {
                            0 => {
                                self.arrowed1 = self.arrowed;
                                self.arrowedpos = 1;
                                self.swap = false;
                            }
                            1 => {
                                self.arrowed2 = self.arrowed;
                                self.arrowedpos = 2;
                            }
                            _ => {
                                self.swap = true;
                                self.arrowedpos = 2;
                            }
                        },
                        VirtualKeyCode::Left => {
                            self.use_slotid = self.arrowed;
                            self.use_item = true;
                        }

                        VirtualKeyCode::U => client.accept_invite(),
                        VirtualKeyCode::I => client.decline_invite(),
                        VirtualKeyCode::T => self.inv_toggle = !self.inv_toggle,
                        VirtualKeyCode::Space => {
                            if self.is_jump_active {
                                client.handle_input(InputKind::Jump, false, None, None);
                                self.is_jump_active = false;
                            } else {
                                client.handle_input(InputKind::Jump, true, None, None);
                                self.is_jump_active = true;
                            }
                        }
                        VirtualKeyCode::X => {
                            if self.is_primary_active {
                                client.handle_input(InputKind::Primary, false, None, None);
                                self.is_primary_active = false;
                            } else {
                                client.handle_input(InputKind::Primary, true, None, None);
                                self.is_primary_active = true;
                            }
                        }
                        VirtualKeyCode::Z => {
                            if self.is_secondary_active {
                                client.handle_input(InputKind::Secondary, false, None, None);
                                self.is_secondary_active = false;
                            } else {
                                client.handle_input(InputKind::Secondary, true, None, None);
                                self.is_secondary_active = true;
                            }
                        }
                        VirtualKeyCode::G => {
                            client.toggle_glide();
                            self.is_glide_active = !self.is_glide_active //do_glide = !do_glide,
                        }
                        VirtualKeyCode::R => client.respawn(),
                        VirtualKeyCode::Plus => self.zoom_level /= 1.5,
                        VirtualKeyCode::Minus => self.zoom_level *= 1.5,

                        _ => {} // Ignore all the other possibilities
                    }
                }
            }

            let mut events = client.tick(inputs, clock.dt(), |_| ()).unwrap();
            let mut inventory_storage = client.state().ecs().read_storage::<comp::Inventory>();
            let mut inventory = inventory_storage.get(client.entity());
            // Tick client
            for event in events {
                match event {
                    Event::Chat(msg) => match msg.chat_type {
                        comp::ChatType::World(_) => self.chat_log.push(msg.message),
                        comp::ChatType::Group(_, _) => {
                            self.chat_log.push(format!("[Group] {}", msg.message))
                        }
                        _ => {}
                    },
                    _ => {}
                }
            }

            // Rendering view
            {
                let state = client.state();
                let level_tint = [0.15, 0.30, 0.50];
                let terrain = state.terrain();

                // Render block
                for y in 0..screen_size.y {
                    for x in 0..screen_size.x {
                        let wpos = (player_pos
                            + Vec3::new(x, y, 0)
                                .map2(screen_size.into(), |e, sz: u16| e as f32 - sz as f32 / 2.0)
                                * Vec2::new(1.0, -1.0)
                                * self.zoom_level)
                            .map(|e| e.floor() as i32);

                        let mut block_z = 0;
                        let mut block = None;
                        let mut block_char = None;

                        let elevation = 16;
                        for (k, z) in (-2..elevation).enumerate() {
                            let block_pos = wpos + Vec3::unit_z() * -z;
                            block_z = block_pos.z;

                            if let Ok(b) = terrain.get(block_pos) {
                                let sprite = b.get_sprite();
                                if sprite.is_some() && sprite.unwrap() != SpriteKind::Empty {
                                    let sprite2 = sprite.unwrap();
                                    let flower1 =
                                        SpriteKind::BarrelCactus as u8..=SpriteKind::Turnip as u8;
                                    let flower2 = SpriteKind::LargeGrass as u8
                                        ..=SpriteKind::LargeCactus as u8;
                                    let furniture = SpriteKind::Window1 as u8
                                        ..=SpriteKind::WardrobeDouble as u8;
                                    block_char = match sprite2 {
                                        SpriteKind::Apple => Some('a'),
                                        SpriteKind::Sunflower => Some('u'),
                                        SpriteKind::Mushroom => Some('m'),
                                        SpriteKind::Velorite | SpriteKind::VeloriteFrag => {
                                            Some('v')
                                        }
                                        SpriteKind::Chest | SpriteKind::Crate => Some('c'),
                                        SpriteKind::Stones => Some('"'),
                                        SpriteKind::Twigs => Some(';'),
                                        SpriteKind::Amethyst | SpriteKind::Ruby => Some('☼'), // TODO: add more
                                        SpriteKind::Beehive => Some('b'),
                                        _ => {
                                            let sprite3 = sprite2 as u8;
                                            if flower1.contains(&sprite3)
                                                || flower2.contains(&sprite3)
                                            {
                                                Some('♣')
                                            } else if furniture.contains(&sprite3) {
                                                match sprite2 {
                                                    SpriteKind::Bed => Some('Θ'),
                                                    SpriteKind::Bench
                                                    | SpriteKind::ChairSingle
                                                    | SpriteKind::ChairDouble => Some('╥'),
                                                    SpriteKind::TableSide
                                                    | SpriteKind::TableDining
                                                    | SpriteKind::TableDouble => Some('╤'),
                                                    _ => Some('π'),
                                                }
                                            } else {
                                                None
                                            }
                                        }
                                    };
                                } else if b.is_filled() {
                                    block = Some(*b);
                                    if block_char.is_none() {
                                        let ublock = block.unwrap();
                                        block_char = match ublock.kind() {
                                            BlockKind::Air => Some(' '),
                                            BlockKind::Water => Some('≈'),
                                            BlockKind::Rock => Some('o'),
                                            BlockKind::WeakRock => Some('.'),
                                            BlockKind::Lava => Some('≈'),
                                            BlockKind::GlowingRock => Some('*'),
                                            BlockKind::GlowingWeakRock => Some('.'),
                                            BlockKind::Grass => Some(','),
                                            BlockKind::Snow => Some('≈'),
                                            BlockKind::Earth => Some('0'),
                                            BlockKind::Sand => Some('▓'),
                                            BlockKind::Wood => Some('≡'),
                                            BlockKind::Leaves => Some('♠'),
                                            BlockKind::Misc => Some('#'),
                                        };
                                    }
                                    break;
                                }
                            }
                        }

                        let level_down = (wpos.z - 2 - block_z) as usize;

                        let col: RGB = match block {
                            Some(block) => match block {
                                _ => {
                                    let rgb = block.get_color().unwrap();

                                    let tint: f32 = if level_down < level_tint.len() {
                                        level_tint[level_down as usize]
                                    } else {
                                        1.0
                                    };

                                    if tint != 1.0 {
                                        RGB::from_u8(
                                            rgb.r + (tint * ((255 - rgb.r) as f32)) as u8,
                                            rgb.g + (tint * ((255 - rgb.g) as f32)) as u8,
                                            rgb.b + (tint * ((255 - rgb.b) as f32)) as u8,
                                        )
                                    } else {
                                        RGB::from_u8(rgb.r, rgb.g, rgb.b)
                                    }
                                }
                            },
                            None => RGB::named(YELLOW),
                        };

                        if block_char.is_none() {
                            block_char = Some('?');
                        }

                        ctx.print_color(x, y, col, RGB::named(BLACK), block_char.unwrap());
                    }
                }

                let objs = state.ecs().entities();
                let positions = state.ecs().read_storage::<comp::Pos>();
                let bodies = state.ecs().read_storage::<comp::Body>();

                for o in objs.join() {
                    let pos = positions.get(o);
                    let body = bodies.get(o);

                    if pos.is_some() && body.is_some() {
                        let scr_pos = to_screen_pos(Vec2::from(pos.unwrap().0), self.zoom_level);
                        let (character, color) = match body.unwrap() {
                            Body::Humanoid(humanoid) => match humanoid.species {
                                humanoid::Species::Danari => ('☻', RGB::named(BROWN2)),
                                humanoid::Species::Dwarf => ('☺', RGB::named(ORANGE)),
                                humanoid::Species::Elf => ('☺', RGB::named(BLUE)),
                                humanoid::Species::Human => ('☺', RGB::named(IVORY)),
                                humanoid::Species::Orc => ('☻', RGB::named(GREEN)),
                                humanoid::Species::Undead => ('☻', RGB::named(WHITE)),
                            },
                            Body::QuadrupedLow(_) => ('4', RGB::named(RED)),
                            Body::QuadrupedSmall(_) => ('q', RGB::named(RED)),
                            Body::QuadrupedMedium(_) => ('Q', RGB::named(RED)),
                            Body::BirdMedium(_) => ('b', RGB::named(RED)),
                            Body::BirdLarge(_) => ('B', RGB::named(RED)),
                            Body::FishSmall(_) => ('f', RGB::named(RED)),
                            Body::FishMedium(_) => ('F', RGB::named(RED)),
                            Body::BipedLarge(_) => ('2', RGB::named(RED)),
                            Body::BipedSmall(_) => ('2', RGB::named(RED)),
                            Body::Object(_) => ('◙', RGB::named(YELLOW)),
                            Body::Golem(_) => ('G', RGB::named(TAN)),
                            Body::Dragon(_) => ('₧', RGB::named(RED)),
                            Body::Theropod(_) => ('T', RGB::named(RED)),
                            Body::Ship(_) => ('S', RGB::named(BROWN1)),
                        };

                        if scr_pos
                            .map2(screen_size, |e, sz| e >= 0 && e < sz as i32)
                            .reduce_and()
                        {
                            ctx.print_color(
                                scr_pos.x,
                                scr_pos.y,
                                color,
                                RGB::named(BLACK),
                                character,
                            );
                        }
                    }
                }
            }

            ctx.printer(
                58,
                screen_size.y - 20,
                "/------- Controls ------\\",
                TextAlign::Right,
                None,
            );

            ctx.printer(
                58,
                screen_size.y - 19,
                "|  wasd/click - Move    |",
                TextAlign::Right,
                None,
            );
            let clear = "                                                                ";
            for (i, msg) in self.chat_log.iter().rev().take(10).enumerate() {
                ctx.printer(
                    58,
                    screen_size.y - 12 - i as u16,
                    clear,
                    TextAlign::Right,
                    None,
                );
                ctx.printer(
                    58,
                    screen_size.y - 12 - i as u16,
                    &format!("#[pink]#[]{}", msg.get(0..48).unwrap_or(&msg)),
                    TextAlign::Right,
                    None,
                );
            }

            ctx.draw_box(39, 0, 20, 5, RGB::named(WHITE), RGB::named(BLACK));
            ctx.printer(
                58,
                1,
                &format!("#[pink]FPS: #[]{}", ctx.fps),
                TextAlign::Right,
                None,
            );
            ctx.printer(
                58,
                2,
                &format!("#[pink]Frame Time: #[]{} ms", ctx.frame_time_ms),
                TextAlign::Right,
                None,
            );
            ctx.printer(
                58,
                3,
                &format!(
                    "#[pink]Health: #[]{}/#[]{}",
                    current_health / 10.0,
                    max_health / 10.0
                ),
                TextAlign::Right,
                None,
            );
            ctx.printer(
                58,
                4,
                &format!(
                    "#[pink]Energy: #[]{}/#[]{}",
                    current_energy / 10.0,
                    max_energy / 10.0
                ),
                TextAlign::Right,
                None,
            );
        }
        client.cleanup();
        // Wait for next tick
        clock.tick();
    }
}

fn read_input() -> String {
    let mut buffer = String::new();

    io::stdin()
        .read_line(&mut buffer)
        .expect("Failed to read input");

    buffer.trim().to_string()
}

fn main() -> BError {
    let view_distance = 12;
    let tps = 60;
    let matches = read_arguments();
    // Find arguments
    let server_addr = matches.value_of("server").unwrap_or("server.veloren.net");
    let server_port = matches.value_of("port").unwrap_or("14004");
    let username = matches.value_of("username").unwrap_or("veloren_user");
    let password = matches.value_of("password").unwrap_or("");
    let character_name = matches.value_of("character").unwrap_or("");

    // Parse server socket
    let mut server_spec = format!("{}:{}", server_addr, server_port);
    let mut server_spec2 = server_spec.clone();
    let runtime = Arc::new(Runtime::new().unwrap());

    let runtime2 = Arc::clone(&runtime);
    let mut client = runtime
        .block_on(async {
            let addr = ConnectionArgs::Tcp {
                hostname: server_spec,
                prefer_ipv6: false,
            };

            let mut mismatched_server_info = None;
            Client::new(
                ConnectionArgs::Tcp {
                    hostname: server_spec2,
                    prefer_ipv6: false,
                },
                Arc::clone(&runtime2),
                &mut mismatched_server_info,
            )
            .await
        })
        .expect("Failed to create client instance");

    println!("Server info: {:?}", client.server_info());
    println!("Players online: {:?}", client.get_players());

    runtime
        .block_on(
            client.register(username.to_string(), password.to_string(), |provider| {
                provider == "https://auth.veloren.net"
            }),
        )
        .unwrap_or_else(|err| {
            println!("Failed to register: {:?}", err);
            process::exit(1);
        });

    // Request character
    let mut clock = Clock::new(Duration::from_secs_f64(1.0 / tps as f64));
    client.load_character_list();

    while client.presence().is_none() {
        assert!(client
            .tick(comp::ControllerInputs::default(), clock.dt(), |_| ())
            .is_ok());
        if client.character_list().characters.len() > 0 {
            let character = client
                .character_list()
                .characters
                .iter()
                .find(|x| x.character.alias == character_name);
            if character.is_some() {
                let character_id = character.unwrap().character.id.unwrap();
                client.request_character(character_id);
                break;
            } else {
                panic!("Character name not found!");
            }
        }
    }

    client.set_view_distance(view_distance);

    let context = BTermBuilder::simple80x50()
        .with_title(&format!("velobracket - {}", character_name))
        .build()?;

    let mut gs = State {
        ecs: World::new(),
        zoom_level: 1.0,
        chat_log: Vec::<String>::new(),
        chat_input: String::new(),
        chat_input_enabled: false,
        inv_toggle: false,
        is_jump_active: false,
        is_secondary_active: false,
        is_primary_active: false,
        is_glide_active: false,
        invpos: 1,
        arrowed1: None,
        arrowed2: None,
        arrowed: None,
        use_slotid: None,
        arrowedpos: 0,
        swap: false,
        use_item: false,
    };

    gs.ecs.insert(client);
    gs.ecs.insert(clock);
    main_loop(context, gs)
}
