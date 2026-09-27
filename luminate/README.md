# `iced_luminate`

The Luminate design kit for [iced](https://iced.rs) 0.14. It provides a theme,
descriptors that `Luminate` turns into elements, and three supporting widgets.
It re-exports `iced_animate`, `iced_texture_cache`, and `iced_page_router`, so
an application needs one dependency line.

## Minimal example

```rust,no_run
use iced_luminate::descriptor::Button;
use iced_luminate::iced::widget::column;
use iced_luminate::iced::{self, Task};
use iced_luminate::theme::typography::FONT;
use iced_luminate::{Element, Luminate, Theme};

struct App {
    luminate: Luminate,
    clicks: u32,
    // Descriptors borrow their text, so the label lives in state.
    label: String,
}

#[derive(Debug, Clone)]
enum Message {
    Clicked,
}

impl App {
    fn new() -> (Self, Task<Message>) {
        let app = Self {
            luminate: Luminate::new(),
            clicks: 0,
            label: String::from("clicked 0"),
        };
        (app, Task::none())
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Clicked => {
                self.clicks += 1;
                self.label = format!("clicked {}", self.clicks);
            }
        }
        Task::none()
    }

    fn view(&self) -> Element<'_, Message> {
        // `host` wraps the root in the motion engine's clock.
        self.luminate.host(column![
            self.luminate.button(Button::new(&self.label).on_press(Message::Clicked)),
        ])
    }
}

fn main() -> iced::Result {
    // Before the application starts: text shaped earlier would not see them.
    Luminate::load_fonts();

    iced::application(App::new, App::update, App::view)
        // The kit's theme is the application's theme (`Theme` is `Copy`).
        .theme(|app: &App| *app.luminate.theme())
        .default_font(FONT)
        .run()
}
```

Add to `Cargo.toml`:

```toml
[dependencies]
iced_luminate = "0.1"
```

## Concepts

### `Luminate`

`Luminate::new()` (or `with_theme(Theme::DARK)`) holds a `Theme` and a `Motion`
engine. It is `Clone`, so pages can keep their own copy. `host(content)`
wraps the root of the view in the engine's clock. Nothing animates without
it. `button`, `input`, `sidebar`, `card`, and `pager` turn descriptors
into `Element`s; `theme()` and `motion()` expose the parts, so a page animates
its own values against `luminate.motion()`.

### Descriptors

`descriptor::Button`, `Input`, `Sidebar`, `Card` and `Pager` are plain data
with public fields and builder methods: `Button::new("Save").hierarchy(
ButtonHierarchy::Secondary).on_press(msg)`, `Input::new(placeholder,
value).label("Name").error(Some("required")).on_input(msg)`,
`Sidebar::new(items).axis(Axis::Vertical).collapsed(false)`,
`Card::new("Title").pages(steps, current).controls(row)`,
`Pager::new(pages).current(i)`.

### Theme

`theme::Theme` is a value of every colour, metric and `TextStyle` the kit
draws with (`Theme::LIGHT`, `Theme::DARK`). It implements `iced::theme::Base`
and the `Catalog`s of the stock widgets (button, text input, container, text,
svg, scrollable, rule, checkbox, toggler, slider, radio, pick list, progress
bar, text editor, combo box) plus the kit's own widgets, so
`iced::application(..).theme(..)` hands one theme to everything and any stock
widget can sit inside an `iced_luminate::Element`, which is typed with it.
Customise it with struct update syntax, such as
`Theme { name: "Mine", ..Theme::LIGHT }`. Give each look a distinct `name`
so iced can detect theme changes. You can also derive a look from a palette
with `Theme::light` or `Theme::dark`.
`theme::palette` holds the colour scales, `theme::typography` the type scale.

### Fonts

The kit ships Inter (variable, upright and italic) under the SIL Open Font
License 1.1 (feature `bundled-font`, on by default). `Luminate::load_fonts()`
registers them with iced's text stack (call it before `iced::application`);
`typography::FONT` is the matching `iced::Font` for `default_font`. The licence
text is at
<https://github.com/sonata-ltd/luminate/blob/master/luminate/src/theme/typography/assets/OFL.txt>.
With `bundled-font` off, `FAMILY` still names the family and you provide the
files.

### Router

`iced_luminate::router` is `iced_page_router`; `iced_luminate::Router` is
`Router<Luminate, Theme, Renderer>`, i.e. pages receive the `Luminate` as their
context. `iced_luminate::texture` is `iced_texture_cache`, and `Cached`, `Pager`
and `TextureCache` are re-exported at the root.

### Widgets

`widget::multi_border::MultiBorder` (`multi_border(..)`),
`widget::sidebar::Sidebar` (`sidebar(..)`) and `widget::error_bubble::ErrorBubble`
(`error_bubble(..)`) are ordinary iced widgets with
`Catalog`s for both `iced::Theme` and `theme::Theme`; the descriptors above are
the usual way in.

## Feature flags

| Feature | Default | Effect |
|---|---|---|
| `bundled-font` | yes | Embeds Inter and enables `Luminate::load_fonts()`. |
| `wgpu`, `tiny-skia` | yes | Backends, passed to `iced` and `iced_texture_cache`. |
| `crisp`, `web-colors`, `thread-pool`, `linux-theme-detection`, `x11`, `wayland` | yes | iced's own defaults, passed through. |
| `svg` | always on | The kit draws icons with `svg`. |
| `image`, `canvas`, `hot`, `strict-assertions` | no | Passed to `iced` (and `iced_texture_cache` where it applies). |

To turn `web-colors` off (gamma-correct linear colour), disable the defaults
and re-list what you need. `iced_luminate` passes the features through to
`iced` and `iced_texture_cache`; if the application also names `iced`
directly, that line must agree with this one:

```toml
[dependencies]
iced_luminate = { version = "0.1", default-features = false, features = ["bundled-font", "svg", "wgpu", "tiny-skia", "thread-pool", "x11", "wayland"] }
# Only if `iced` is a direct dependency as well:
iced = { version = "0.14", default-features = false, features = ["wgpu", "tiny-skia", "thread-pool", "x11", "wayland"] }
```

## Limitations

* The theme is a value, not a trait: custom looks are a modified `Theme`, not
  a new type. `Theme::DARK` is a first pass: the light metrics on dark
  surfaces, checked for contrast but not yet designed.
* Descriptors cover the kit's own vocabulary; anything else is a stock iced
  widget typed with `iced_luminate::Theme`.
* Rotation is not an animation tier (the compositor blits axis-aligned).

## Related crates

* [`iced_animate`](https://crates.io/crates/iced_animate),
  [`iced_texture_cache`](https://crates.io/crates/iced_texture_cache),
  [`iced_page_router`](https://crates.io/crates/iced_page_router) are the
  underlying crates. Each works on its own.

Example: `cargo run -p iced_luminate --example overview` (no environment
variables). It lives in the
[repository](https://github.com/sonata-ltd/luminate/tree/master/luminate/examples)
and is not part of the published crate. Changes are listed in the workspace
[CHANGELOG](https://github.com/sonata-ltd/luminate/blob/master/CHANGELOG.md).
