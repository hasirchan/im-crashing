# im-crashing!

**A very basic prototype input method front end for my personal use, based on the Rime engine.**

Failtures:
- Supports Wayland only; your compositor must support the Input Method v2 unstable and Virtual Keyboard v1 unstable protocols
- Does not render any graphics, including the candidate window
- But we still offer a really cool way to render the candidates!
- However, if your app doesn't handle preedit text correctly, you really won't be able to see any candidates!
- Error handling is very crude; it crashes immediately upon encountering any unexpected situation
- Error message: filename + line number + traceback
- Small binary build
- Poor coding style
- No more guidelines

By the way, [this](https://github.com/xhebox/wlpinyin) should be much more reliable. After all, I’ve ~~copied~~ borrowed a few from here.
