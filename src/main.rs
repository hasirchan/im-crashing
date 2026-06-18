#[macro_export]
macro_rules! lazy_err {
    () => {{
        let backtrace = std::backtrace::Backtrace::force_capture();
        panic!("`lazy_err!` called, backtrace:\n{}", backtrace);
    }};
}

mod app;
mod rime;
mod ffi {
    pub mod rime {
        include!(concat!(env!("OUT_DIR"), "/rime_bindings.rs"));
    }
}

fn main() {
    let mut app = crate::app::App::init();
    app.run();
}
