//! Button hierarchies, and `Action::navigate_with`: "Go to inputs" carries
//! `NavigationOptions` to the inputs page.

use iced::Length;
use iced::widget::{container, row, svg};
use iced_luminate::descriptor::{Button, ButtonHierarchy};
use iced_luminate::iced::widget::column;
use iced_luminate::router::{Action, Page, Registry};
use iced_luminate::{Element, Luminate, Renderer, Theme};

use crate::hero::Hero;
use crate::iso::scenes_flat;

/// The play icon every button on the page shows.
///
/// From memory, never from a `&str`: iced turns a string into a handle
/// through `Into<PathBuf>`, so passing the markup itself reads it as a file
/// name and draws nothing.
fn icon() -> svg::Handle {
    svg::Handle::from_memory(include_bytes!("../assets/play.svg"))
}

/// Messages of the buttons page.
#[derive(Debug, Clone)]
pub(crate) enum Message {
    /// A plain action button was pressed.
    ActionPressed,
}

/// Three hierarchies of the same button.
pub(crate) struct ButtonsPage {
    luminate: Luminate,
}

impl Page for ButtonsPage {
    type Message = Message;
    type NavigationOptions = ();
    type Context = Luminate;
    type Theme = Theme;
    type Renderer = Renderer;

    fn new(luminate: &Luminate, _: &Registry) -> Self {
        Self {
            luminate: luminate.clone(),
        }
    }

    fn update(&mut self, message: Message) -> Action<Message> {
        match message {
            Message::ActionPressed => {
                eprintln!("overview: action pressed");
                Action::none()
            }
        }
    }

    fn view(&self) -> Element<'_, Message> {
        let luminate = &self.luminate;
        let theme = luminate.theme();

        let icon = icon();
        let column_spacing = theme.spacing.lg;

        Hero::new(
            luminate,
            &scenes_flat::BUTTONS,
            "Buttons",
            "Three hierarchies of one control",
            container(
                row![
                    column![
                        luminate.button(
                            Button::with_icon(icon.clone()).on_press(Message::ActionPressed)
                        ),
                        luminate.button(
                            Button::with_icon(icon.clone())
                                .hierarchy(ButtonHierarchy::Secondary)
                                .on_press(Message::ActionPressed)
                        ),
                        luminate.button(
                            Button::with_icon(icon.clone())
                                .hierarchy(ButtonHierarchy::Tertiary)
                                .on_press(Message::ActionPressed)
                        ),
                    ]
                    .spacing(column_spacing),
                    column![
                        luminate.button(Button::new("Action").on_press(Message::ActionPressed)),
                        luminate.button(
                            Button::new("Action")
                                .hierarchy(ButtonHierarchy::Secondary)
                                .on_press(Message::ActionPressed)
                        ),
                        luminate.button(
                            Button::new("Action")
                                .hierarchy(ButtonHierarchy::Tertiary)
                                .on_press(Message::ActionPressed)
                        ),
                    ]
                    .spacing(column_spacing),
                    column![
                        luminate.button(
                            Button::new("Action")
                                .icon(icon.clone())
                                .on_press(Message::ActionPressed)
                        ),
                        luminate.button(
                            Button::new("Action")
                                .icon(icon.clone())
                                .hierarchy(ButtonHierarchy::Secondary)
                                .on_press(Message::ActionPressed)
                        ),
                        luminate.button(
                            Button::new("Action")
                                .icon(icon.clone())
                                .hierarchy(ButtonHierarchy::Tertiary)
                                .on_press(Message::ActionPressed)
                        ),
                    ]
                    .spacing(column_spacing),
                ]
                .spacing(theme.spacing.lg),
            )
            .center_x(Length::Fill),
        )
        .build()
    }
}

#[cfg(test)]
mod tests {
    use iced_luminate::iced::advanced::Renderer as _;
    use iced_luminate::iced::advanced::clipboard;
    use iced_luminate::iced::advanced::renderer::{self, Headless};
    use iced_luminate::iced::time::Instant;
    use iced_luminate::iced::{Color, Event, Rectangle, Size, mouse, window};
    use iced_luminate::texture::testing::headless_tiny_skia;
    use iced_test::runtime::user_interface::{self, UserInterface};

    use super::*;

    const SIZE: Size = Size::new(64.0, 64.0);

    /// An icon-only primary button showing `icon`, drawn on the software
    /// backend.
    fn render(icon: svg::Handle) -> Vec<u8> {
        let luminate = Luminate::default();
        let root: Element<'_, ()> = luminate.button(Button::with_icon(icon).on_press(()));
        let mut renderer = headless_tiny_skia();
        let mut ui =
            UserInterface::build(root, SIZE, user_interface::Cache::default(), &mut renderer);
        let _ = ui.update(
            &[Event::Window(
                window::Event::RedrawRequested(Instant::now()),
            )],
            mouse::Cursor::Unavailable,
            &mut renderer,
            &mut clipboard::Null,
            &mut Vec::new(),
        );
        renderer.reset(Rectangle::with_size(SIZE));
        ui.draw(
            &mut renderer,
            luminate.theme(),
            &renderer::Style {
                text_color: Color::BLACK,
            },
            mouse::Cursor::Unavailable,
        );
        renderer.screenshot(Size::new(64, 64), 1.0, Color::WHITE)
    }

    /// The icons were handed over as markup in a `&str`, which iced reads
    /// as a path: nothing was found, and the buttons drew no icon at all.
    #[test]
    fn the_page_icon_draws() {
        let blank = svg::Handle::from_memory(
            br#"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24"/>"#.as_slice(),
        );

        assert!(render(icon()) != render(blank), "the icon left no mark");
    }
}
