use crate::rime::{self};
use rustix::{
    buffer::spare_capacity,
    event::epoll,
    fd::{AsFd, AsRawFd, FromRawFd, OwnedFd},
};
use std::collections::{
    HashMap, HashSet,
    hash_map::{self},
};
use wayland_client::{
    Connection as WlConnection, Dispatch as WlDispatch, EventQueue as WlEventQueue,
    Proxy as WlProxy, QueueHandle as WlQueueHandle, WEnum as WlWEnum,
    globals::{
        GlobalListContents as WlGlobalListContents, registry_queue_init as wl_registry_queue_init,
    },
    protocol::{
        wl_keyboard::{KeyState as WlKeyState, KeymapFormat as WlKeymapFormat},
        wl_registry::{Event as WlRegEvent, WlRegistry},
        wl_seat::{Event as WlSeatEvent, WlSeat},
    },
};
use wayland_protocols_misc::{
    zwp_input_method_v2::client::{
        zwp_input_method_keyboard_grab_v2::{
            Event as WlIMKbdGrabEvent, ZwpInputMethodKeyboardGrabV2 as WlIMKbdGrab,
        },
        zwp_input_method_manager_v2::{
            Event as WlIMManagerEvent, ZwpInputMethodManagerV2 as WlIMManager,
        },
        zwp_input_method_v2::{self, Event as WlIMEvent, ZwpInputMethodV2 as WlIM},
    },
    zwp_virtual_keyboard_v1::client::{
        zwp_virtual_keyboard_manager_v1::{
            Event as WlVKManagerEvent, ZwpVirtualKeyboardManagerV1 as WlVKManager,
        },
        zwp_virtual_keyboard_v1::{Event as WlVKEvent, ZwpVirtualKeyboardV1 as WlVK},
    },
};
use xkbcommon::xkb::{self};

const RIME_RELEASE_MASK: u32 = 1 << 30;

fn create_signal_fd(signals: &[std::ffi::c_int]) -> OwnedFd {
    unsafe {
        let mut mask = std::mem::zeroed();
        if libc::sigemptyset(&mut mask) == -1 {
            lazy_err!()
        }
        for &sig in signals {
            if libc::sigaddset(&mut mask, sig) == -1 {
                lazy_err!()
            }
        }
        if libc::sigprocmask(libc::SIG_BLOCK, &mask, std::ptr::null_mut()) == -1 {
            lazy_err!()
        }
        let fd = libc::signalfd(-1, &mask, libc::SFD_CLOEXEC);
        if fd == -1 {
            lazy_err!()
        }
        OwnedFd::from_raw_fd(fd)
    }
}

pub struct App {
    state: AppState,
    queue: WlEventQueue<AppState>,
}

impl App {
    pub fn init() -> Self {
        let (state, queue) = AppState::init();
        Self { state, queue }
    }

    pub fn run(&mut self) {
        let signal_fd = create_signal_fd(&[libc::SIGTERM, libc::SIGINT]);
        let epoll_fd = epoll::create(epoll::CreateFlags::CLOEXEC).unwrap();
        epoll::add(
            &epoll_fd,
            self.queue.as_fd(),
            epoll::EventData::new_u64(0),
            epoll::EventFlags::IN,
        )
        .unwrap();
        epoll::add(
            &epoll_fd,
            &signal_fd,
            epoll::EventData::new_u64(1),
            epoll::EventFlags::IN,
        )
        .unwrap();
        let mut event_buf: Vec<epoll::Event> = Vec::with_capacity(4);
        loop {
            self.queue.flush().unwrap();
            let Some(guard) = self.queue.prepare_read() else {
                self.queue.dispatch_pending(&mut self.state).unwrap();
                continue;
            };
            let mut prepare_fk_rustc = Some(guard);
            epoll::wait(&epoll_fd, spare_capacity(&mut event_buf), None).unwrap();
            for event in event_buf.drain(..) {
                match event.data.u64() {
                    0 => {
                        let Some(do_fk_rustc) = prepare_fk_rustc.take() else {
                            continue;
                        };
                        do_fk_rustc.read().unwrap();
                        self.queue.dispatch_pending(&mut self.state).unwrap();
                    }
                    1 => {
                        return;
                    }
                    _ => {}
                }
            }
        }
    }
}

struct InputMethod {
    activated: bool,
    seat: WlSeat,
    vk: WlVK,
    im: WlIM,
    event_pending: Vec<WlIMEvent>,
    kbd_grab: WlIMKbdGrab,
    xkb_keymap: Option<xkb::Keymap>,
    xkb_state: Option<xkb::State>,
    key_handled: HashSet<u32>,
    rime_session_id: rime::RimeSessionId,
    serial: u32,
}

impl InputMethod {
    fn init(seat: WlSeat, im: WlIM, vk: WlVK, kbd_grab: WlIMKbdGrab) -> Self {
        rime::Rime::init();
        InputMethod {
            activated: false,
            seat,
            vk,
            im,
            kbd_grab,
            event_pending: vec![],
            xkb_keymap: None,
            xkb_state: None,
            key_handled: HashSet::new(),
            rime_session_id: rime::Rime::create_session(),
            serial: 0,
        }
    }

    fn handle_preedit(&mut self) {
        let ctx = rime::Rime::get_context(&self.rime_session_id).unwrap();
        let preedit = match ctx.composition.preedit.as_deref() {
            None | Some("") => {
                self.im.set_preedit_string(String::new(), 0, 0);
                return;
            }
            Some(s) => s,
        };
        let mut buf = String::new();
        buf.push_str(preedit);
        let cursor = buf.len().try_into().unwrap();
        for (i, candidate) in ctx.menu.candidates.iter().enumerate() {
            let highlighted = i
                .try_into()
                .map(|c_int_i: std::ffi::c_int| c_int_i == ctx.menu.highlighted_candidate_index)
                .unwrap();
            if highlighted {
                buf.push_str(&format!(" [{}. {}]", i + 1, candidate.text));
            } else {
                buf.push_str(&format!(" {}. {}", i + 1, candidate.text));
            }
        }
        self.im.set_preedit_string(buf, cursor, cursor);
    }

    fn handle_key(&mut self, key_raw: u32, key_state: WlWEnum<WlKeyState>) -> bool {
        let key_xkb = xkb::Keycode::new(key_raw + 8);
        let key_final = self.xkb_state.as_mut().unwrap().key_get_one_sym(key_xkb);
        match key_state {
            WlWEnum::Value(WlKeyState::Pressed) => {
                self.xkb_state
                    .as_mut()
                    .unwrap()
                    .update_key(key_xkb, xkb::KeyDirection::Down);
            }
            WlWEnum::Value(WlKeyState::Released) => {
                self.xkb_state
                    .as_mut()
                    .unwrap()
                    .update_key(key_xkb, xkb::KeyDirection::Up);
            }
            _ => return false,
        };
        if !self.activated {
            return false;
        }

        let mut mods = self
            .xkb_state
            .as_mut()
            .unwrap()
            .serialize_mods(xkb::STATE_MODS_EFFECTIVE | xkb::STATE_LAYOUT_EFFECTIVE);

        if key_state == WlWEnum::Value(WlKeyState::Released) {
            mods |= RIME_RELEASE_MASK;
        }

        match rime::Rime::process_key(&self.rime_session_id, key_final, mods) {
            true => {
                self.handle_preedit();
                if let Some(commit) = rime::Rime::get_commit(&self.rime_session_id) {
                    self.im.commit_string(commit.text);
                }
                self.im.commit(self.serial);
                self.key_handled.insert(key_raw);
                true
            }
            false => self.key_handled.remove(&key_raw),
        }
    }
}

impl Drop for InputMethod {
    fn drop(&mut self) {
        rime::Rime::destroy_session(&self.rime_session_id);
        rime::Rime::deinit();
    }
}

struct AppState {
    im_map: HashMap<u32, InputMethod>,
    im_manager: WlIMManager,
    vk_manager: WlVKManager,
    xkb_context: xkb::Context,
    wl_conn: WlConnection,
}

impl AppState {
    fn update_im_map(
        &mut self,
        name: u32,
        version: Option<u32>,
        registry: Option<&WlRegistry>,
        queue_handle: Option<&WlQueueHandle<Self>>,
    ) {
        let potential_im = match (registry, version, queue_handle) {
            (Some(reg), Some(ver), Some(qh)) => {
                let seat = reg.bind(name, ver, qh, ());
                let im = self.im_manager.get_input_method(&seat, qh, name);
                let vk = self.vk_manager.create_virtual_keyboard(&seat, qh, ());
                let kbd_grab = im.grab_keyboard(qh, name);
                Some(InputMethod::init(seat, im, vk, kbd_grab))
            }
            _ => None,
        };
        match (self.im_map.entry(name), potential_im) {
            (hash_map::Entry::Vacant(entry), Some(im)) => {
                entry.insert(im);
            }
            (hash_map::Entry::Vacant(_), None) => {}
            (hash_map::Entry::Occupied(mut entry), Some(im)) => {
                entry.insert(im);
            }
            (hash_map::Entry::Occupied(entry), None) => {
                entry.remove();
            }
        }
    }

    fn init() -> (Self, WlEventQueue<Self>) {
        rime::Rime::init();
        let conn = WlConnection::connect_to_env().unwrap();
        let (global_list, queue) = wl_registry_queue_init::<Self>(&conn).unwrap();
        let qh = queue.handle();
        let im_manager: WlIMManager = global_list
            .bind(&qh, 1..=WlIMManager::interface().version, ())
            .unwrap();
        let vk_manager: WlVKManager = global_list
            .bind(&qh, 1..=WlVKManager::interface().version, ())
            .unwrap();
        let im_map = HashMap::new();
        let mut app_state = Self {
            im_map,
            im_manager,
            vk_manager,
            xkb_context: xkb::Context::new(xkb::CONTEXT_NO_FLAGS),
            wl_conn: conn,
        };
        let registry = global_list.registry();
        global_list.contents().with_list(|globals| {
            globals.iter().for_each(|g| {
                if g.interface != WlSeat::interface().name {
                    return;
                }
                app_state.update_im_map(g.name, Some(g.version), Some(registry), Some(&qh));
            });
        });
        (app_state, queue)
    }
}

impl WlDispatch<WlIM, u32> for AppState {
    fn event(
        app_state: &mut Self,
        _: &WlIM,
        event: zwp_input_method_v2::Event,
        seat_name: &u32,
        _: &WlConnection,
        qh: &WlQueueHandle<Self>,
    ) {
        match event {
            WlIMEvent::Done => {
                let im = app_state.im_map.get_mut(seat_name).unwrap();
                for event in im.event_pending.drain(..) {
                    match event {
                        WlIMEvent::Deactivate => {
                            rime::Rime::clear_composition(&im.rime_session_id);
                            im.key_handled.clear();
                            im.activated = false;
                        }
                        WlIMEvent::Activate => {
                            rime::Rime::clear_composition(&im.rime_session_id);
                            im.key_handled.clear();
                            im.activated = true;
                        }
                        WlIMEvent::ContentType { hint, purpose } => {}
                        WlIMEvent::TextChangeCause { cause } => {}
                        WlIMEvent::SurroundingText {
                            text,
                            cursor,
                            anchor,
                        } => {}
                        _ => {}
                    }
                }
                im.serial += 1;
            }
            WlIMEvent::Unavailable => {
                lazy_err!()
            }
            _ => {
                let im = app_state.im_map.get_mut(seat_name).unwrap();
                im.event_pending.push(event);
            }
        }
        app_state.wl_conn.flush().unwrap();
    }
}

impl WlDispatch<WlIMKbdGrab, u32> for AppState {
    fn event(
        app_state: &mut Self,
        _: &WlIMKbdGrab,
        event: WlIMKbdGrabEvent,
        seat_name: &u32,
        _: &WlConnection,
        _: &WlQueueHandle<Self>,
    ) {
        match event {
            WlIMKbdGrabEvent::Key {
                serial,
                state,
                time,
                key,
            } => {
                let im = app_state.im_map.get_mut(seat_name).unwrap();
                if im.handle_key(key, state) {
                    return;
                }
                im.vk.key(time, key, state.into());
            }
            WlIMKbdGrabEvent::Modifiers {
                serial,
                mods_depressed,
                mods_latched,
                mods_locked,
                group,
            } => {
                let im = app_state.im_map.get_mut(seat_name).unwrap();
                im.vk
                    .modifiers(mods_depressed, mods_latched, mods_locked, group);
            }
            WlIMKbdGrabEvent::Keymap { format, fd, size } => {
                if format != WlWEnum::Value(WlKeymapFormat::XkbV1) {
                    lazy_err!()
                }
                let im = app_state.im_map.get_mut(seat_name).unwrap();
                im.vk.keymap(format.into(), fd.as_fd(), size);
                let keymap_str = unsafe {
                    let ptr = libc::mmap(
                        std::ptr::null_mut(),
                        size.try_into().unwrap(),
                        libc::PROT_READ,
                        libc::MAP_PRIVATE,
                        fd.as_raw_fd(),
                        0,
                    );
                    if ptr == libc::MAP_FAILED {
                        app_state.update_im_map(*seat_name, None, None, None);
                        return;
                    }
                    let bytes =
                        std::slice::from_raw_parts(ptr as *const u8, size.try_into().unwrap());
                    let s = std::str::from_utf8(bytes).unwrap().to_owned();
                    libc::munmap(ptr, size.try_into().unwrap());
                    s
                };
                let keymap = xkb::Keymap::new_from_string(
                    &app_state.xkb_context,
                    keymap_str,
                    xkb::KEYMAP_FORMAT_TEXT_V1,
                    xkb::COMPILE_NO_FLAGS,
                )
                .unwrap();
                im.xkb_state = Some(xkb::State::new(&keymap));
                im.xkb_keymap = Some(keymap);
            }
            WlIMKbdGrabEvent::RepeatInfo { rate, delay } => {}
            _ => {}
        }
        app_state.wl_conn.flush().unwrap();
    }
}

impl WlDispatch<WlRegistry, WlGlobalListContents> for AppState {
    fn event(
        app_state: &mut Self,
        registry: &WlRegistry,
        event: WlRegEvent,
        _: &WlGlobalListContents,
        _: &WlConnection,
        queue_handle: &WlQueueHandle<Self>,
    ) {
        match event {
            WlRegEvent::Global {
                name,
                interface,
                version,
            } => {
                if interface != WlSeat::interface().name {
                    return;
                };
                app_state.update_im_map(name, Some(version), Some(registry), Some(queue_handle));
            }
            WlRegEvent::GlobalRemove { name } => {
                app_state.update_im_map(name, None, None, None);
            }
            _ => {}
        }
        app_state.wl_conn.flush().unwrap();
    }
}

impl WlDispatch<WlSeat, ()> for AppState {
    fn event(
        _: &mut Self,
        _: &WlSeat,
        _: WlSeatEvent,
        _: &(),
        _: &WlConnection,
        _: &WlQueueHandle<Self>,
    ) {
    }
}

impl WlDispatch<WlIMManager, ()> for AppState {
    fn event(
        _: &mut Self,
        _: &WlIMManager,
        _: WlIMManagerEvent,
        _: &(),
        _: &WlConnection,
        _: &WlQueueHandle<Self>,
    ) {
    }
}

impl WlDispatch<WlVKManager, ()> for AppState {
    fn event(
        _: &mut Self,
        _: &WlVKManager,
        _: WlVKManagerEvent,
        _: &(),
        _: &WlConnection,
        _: &WlQueueHandle<Self>,
    ) {
    }
}

impl WlDispatch<WlVK, ()> for AppState {
    fn event(
        _: &mut Self,
        _: &WlVK,
        _: WlVKEvent,
        _: &(),
        _: &WlConnection,
        _: &WlQueueHandle<Self>,
    ) {
    }
}
