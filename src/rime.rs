use crate::ffi::rime::{self as raw_rime};
use std::{
    ffi::{CStr, CString, c_char, c_int},
    sync::atomic::{AtomicBool, AtomicPtr, AtomicUsize, Ordering},
};
use xkbcommon::xkb::{self};

static RIME_TRAITS: AtomicPtr<raw_rime::RimeTraits> = AtomicPtr::new(std::ptr::null_mut());
// overdesigned, but fun
static RIME_RC: AtomicUsize = AtomicUsize::new(0);
static RIME_IS_INIT: AtomicBool = AtomicBool::new(false);

macro_rules! rime_struct_init {
    ($type:ty) => {{
        let mut s: $type = unsafe { std::mem::zeroed() };
        s.data_size = (std::mem::size_of::<$type>() - std::mem::size_of::<c_int>())
            .try_into()
            .unwrap();
        s
    }};
}

macro_rules! rime_call {
    ($func:ident $(, $arg:expr)*) => {
        unsafe {
            (*raw_rime::rime_get_api()).$func.expect(stringify!($func))($($arg),*)
        }
    };
}

fn ptr_to_string(ptr: *const c_char) -> Option<String> {
    if ptr.is_null() {
        return None;
    }
    let bytes = unsafe { CStr::from_ptr(ptr) }.to_bytes();
    let res = std::str::from_utf8(bytes).unwrap();
    Some(res.to_owned())
}

fn string_to_cstring(string: &str) -> CString {
    std::ffi::CString::new(string).unwrap()
}

pub struct RimeSessionId {
    raw: usize,
}

pub struct RimeCommit {
    pub text: String,
}

impl From<&raw_rime::RimeCommit> for RimeCommit {
    fn from(c: &raw_rime::RimeCommit) -> RimeCommit {
        RimeCommit {
            text: ptr_to_string(c.text).unwrap(),
        }
    }
}

pub struct RimeCandidate {
    pub text: String,
    comment: Option<String>,
}

impl From<&raw_rime::RimeCandidate> for RimeCandidate {
    fn from(c: &raw_rime::RimeCandidate) -> RimeCandidate {
        Self {
            text: ptr_to_string(c.text).unwrap_or_default(),
            comment: ptr_to_string(c.comment),
        }
    }
}

pub struct RimeMenu {
    pub page_size: c_int,
    pub page_no: c_int,
    is_last_page: bool,
    pub highlighted_candidate_index: c_int,
    pub candidates: Vec<RimeCandidate>,
    select_keys: Option<String>,
}

impl From<&raw_rime::RimeMenu> for RimeMenu {
    fn from(m: &raw_rime::RimeMenu) -> Self {
        let mut res = Self {
            page_size: m.page_size,
            page_no: m.page_no,
            is_last_page: m.is_last_page == 1,
            highlighted_candidate_index: m.highlighted_candidate_index,
            candidates: vec![],
            select_keys: ptr_to_string(m.select_keys),
        };
        if m.candidates.is_null() || m.num_candidates <= 0 {
            return res;
        };
        res.candidates = unsafe {
            std::slice::from_raw_parts(m.candidates, m.num_candidates.try_into().unwrap())
        }
        .iter()
        .map(RimeCandidate::from)
        .collect();
        res
    }
}

pub struct RimeComposition {
    pub length: c_int,
    pub cursor_pos: c_int,
    pub sel_start: c_int,
    pub sel_end: c_int,
    pub preedit: Option<String>,
}

impl From<&raw_rime::RimeComposition> for RimeComposition {
    fn from(c: &raw_rime::RimeComposition) -> Self {
        Self {
            length: c.length,
            cursor_pos: c.cursor_pos,
            sel_start: c.sel_start,
            sel_end: c.sel_end,
            preedit: ptr_to_string(c.preedit),
        }
    }
}

pub struct RimeContext {
    pub composition: RimeComposition,
    pub menu: RimeMenu,
    commit_text_preview: Option<String>,
    select_labels: Vec<String>,
}

impl From<&raw_rime::RimeContext> for RimeContext {
    fn from(ctx: &raw_rime::RimeContext) -> Self {
        let mut res = Self {
            composition: RimeComposition::from(&ctx.composition),
            menu: RimeMenu::from(&ctx.menu),
            commit_text_preview: ptr_to_string(ctx.commit_text_preview),
            select_labels: vec![],
        };
        if ctx.select_labels.is_null() {
            return res;
        }
        let mut i = 0;
        loop {
            let ptr = unsafe { *ctx.select_labels.add(i) };
            if ptr.is_null() {
                break;
            }
            res.select_labels.push(ptr_to_string(ptr).unwrap());
            i += 1;
        }
        res
    }
}

pub struct Rime;

impl Rime {
    pub fn init() {
        RIME_RC.fetch_add(1, Ordering::SeqCst);
        if RIME_IS_INIT.swap(true, Ordering::SeqCst) {
            return;
        }
        let pkg_name = env!("CARGO_PKG_NAME");
        let pkg_version = env!("CARGO_PKG_VERSION");

        let home = std::env::var("HOME").unwrap();
        let user_data_dir = format!("{}/.local/share/{}/rime", home, pkg_name);
        let log_dir = format!("{}/.local/share/{}/log", home, pkg_name);

        println!("{}", env!("RIME_SHARED_DATA_DIR"));
        std::fs::create_dir_all(&user_data_dir).ok();
        std::fs::create_dir_all(&log_dir).ok();

        let mut traits = rime_struct_init!(raw_rime::RimeTraits);
        traits.shared_data_dir = string_to_cstring(env!("RIME_SHARED_DATA_DIR")).into_raw();
        traits.user_data_dir = string_to_cstring(user_data_dir.as_str()).into_raw();
        traits.app_name = string_to_cstring(format!("rime.{}", pkg_name).as_str()).into_raw();
        traits.log_dir = string_to_cstring(log_dir.as_str()).into_raw();
        traits.distribution_name = string_to_cstring(pkg_name).into_raw();
        traits.distribution_code_name = string_to_cstring(pkg_name).into_raw();
        traits.distribution_version = string_to_cstring(pkg_version).into_raw();

        rime_call!(setup, &mut traits);
        rime_call!(initialize, &mut traits);
        if rime_call!(start_maintenance, 1) == 0 {
            lazy_err!()
        }
        rime_call!(join_maintenance_thread);

        let traits_raw: *mut raw_rime::RimeTraits = Box::into_raw(Box::new(traits));
        RIME_TRAITS.store(traits_raw, Ordering::Relaxed);
    }

    pub fn deinit() {
        if !RIME_IS_INIT.load(Ordering::SeqCst) {
            return;
        }
        if RIME_RC.fetch_sub(1, Ordering::SeqCst) - 1 > 0 {
            return;
        }
        let traits_raw = RIME_TRAITS.load(Ordering::Relaxed);
        if traits_raw.is_null() {
            return;
        }
        rime_call!(cleanup_all_sessions);
        rime_call!(finalize);
        unsafe {
            let traits = Box::from_raw(traits_raw);
            if !traits.shared_data_dir.is_null() {
                drop(CString::from_raw(traits.shared_data_dir as *mut _));
            }
            if !traits.user_data_dir.is_null() {
                drop(CString::from_raw(traits.user_data_dir as *mut _));
            }
            if !traits.app_name.is_null() {
                drop(CString::from_raw(traits.app_name as *mut _));
            }
            if !traits.log_dir.is_null() {
                drop(CString::from_raw(traits.log_dir as *mut _));
            }
            if !traits.distribution_name.is_null() {
                drop(CString::from_raw(traits.distribution_name as *mut _));
            }
            if !traits.distribution_code_name.is_null() {
                drop(CString::from_raw(traits.distribution_code_name as *mut _));
            }
            if !traits.distribution_version.is_null() {
                drop(CString::from_raw(traits.distribution_version as *mut _));
            }
        };
        RIME_TRAITS.store(std::ptr::null_mut(), Ordering::Relaxed);
    }

    fn is_init() -> bool {
        RIME_IS_INIT.load(Ordering::Relaxed)
    }

    pub fn get_commit(session_id: &RimeSessionId) -> Option<RimeCommit> {
        if !Self::is_init() {
            lazy_err!()
        }
        let mut commit = rime_struct_init!(raw_rime::RimeCommit);
        if rime_call!(get_commit, session_id.raw, &mut commit) == 0 {
            return None;
        }
        let res = RimeCommit::from(&commit);
        if rime_call!(free_commit, &mut commit) == 0 {
            lazy_err!()
        }
        Some(res)
    }

    pub fn get_context(session_id: &RimeSessionId) -> Option<RimeContext> {
        if !Self::is_init() {
            lazy_err!()
        }
        let mut context = rime_struct_init!(raw_rime::RimeContext);
        if rime_call!(get_context, session_id.raw, &mut context) == 0 {
            return None;
        }
        let res = RimeContext::from(&context);
        if rime_call!(free_context, &mut context) == 0 {
            lazy_err!()
        }
        Some(res)
    }

    pub fn clear_composition(session_id: &RimeSessionId) {
        if !Self::is_init() {
            lazy_err!()
        }
        rime_call!(clear_composition, session_id.raw);
    }

    pub fn destroy_session(session_id: &RimeSessionId) {
        if !Self::is_init() {
            lazy_err!()
        }
        if rime_call!(destroy_session, session_id.raw) == 0 {
            lazy_err!()
        }
    }

    pub fn create_session() -> RimeSessionId {
        if !Self::is_init() {
            lazy_err!()
        }
        RimeSessionId {
            raw: rime_call!(create_session),
        }
    }

    pub fn process_key(session_id: &RimeSessionId, key: xkb::Keysym, mods: xkb::ModMask) -> bool {
        rime_call!(
            process_key,
            session_id.raw,
            key.raw().try_into().unwrap(),
            mods.try_into().unwrap()
        ) == 1
    }
}
