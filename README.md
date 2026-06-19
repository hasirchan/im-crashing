# im-crashing!

**A very basic prototype input method front end for my personal use, based on the Rime engine.**

---

## Failtures
- Supports Wayland only. Your compositor must support the Input Method v2 unstable and Virtual Keyboard v1 unstable protocols
- Does not render any graphics, including the candidate window
- But we still offer a really cool way to render the candidates!
- However, if your app doesn't handle preedit text correctly, you really won't be able to see any candidates!
- Error handling is very crude. It crashes immediately upon encountering any unexpected situation
- Error message: filename + line number + traceback
- Small binary build
- Poor coding style
- No more guidelines

By the way, [this](https://github.com/xhebox/wlpinyin) should be much more reliable. After all, I’ve ~~copied~~ borrowed a few from here.

---

## Building and Running

```
git clone https://github.com/hasirchan/im-crashing.git
cd im-crashing
nix develop (or direnv allow)
cargo build --release
./target/release/im-crashing
```

If you are not using the Nix package manager, you may need to manually set the `RIME_SHARED_DATA_DIR` environment variable when compiling `im-crashing` so that the `rime` engine can locate the default configuration file.

`im-crashing` itself has no configuration options or logs. All configurable options are provided by `rime`, and logs are generated solely by `rime`. The default paths are `$HOME/.local/share/im-crashing/rime` and `$HOME/.local/share/im-crashing/log`.

---

## Note
- If you have another input method running, `im-crashing` may crash immediately at startup, as the presence of another running input method will cause `im-crashing` to become unavailable.
- If `im-crashing` does not detect the `HOME` environment variable at startup, it will crash immediately because it expects to store runtime data and configuration files in a directory under `HOME`.
- If you're using a compositor from the `wlroots` ecosystem, `im-crashing` will most likely work as expected, but this ultimately depends on your runtime environment and the target application.
- If you find that `im-crashing` does not work for specific applications (e.g., you can’t see the candidates, or `im-crashing` is completely unavailable), this is likely not a problem with `im-crashing` itself, you may be encountering a compatibility issue. If you are using a compositor from the ecosystem mentioned above, it is almost certainly an issue with your runtime environment (such as a tampered `GTK_IM_MODULE` or similar environment variable) or the target application (such as certain older games), rather than an issue with `im-crashing` itself. There's nothing I can do about it.
