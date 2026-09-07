//! [`Luminate::card`] and the layout widget behind it.

use iced::advanced::layout::{Layout, Limits, Node};
use iced::advanced::widget::{Operation, Tree};
use iced::advanced::{Clipboard, Shell, Widget, mouse, overlay, renderer};
use iced::widget::{Space, container};
use iced::{Event, Length, Point, Rectangle, Size, Vector};
use iced_animate::curves;
use iced_texture_cache::{Cached, Pager, PixelSnap};

use crate::descriptor::Card;
use crate::luminate::Luminate;
use crate::theme::ContainerClass;
use crate::theme::typography::styled_text;
use crate::{Element, Renderer, Theme};

impl Luminate {
    /// Builds a card: a titled header, a sliding page stack and an optional
    /// controls row on the theme's card surface with its shadows. With
    /// `max_height` the page stack shrinks so the header and the controls
    /// stay fully visible.
    #[must_use]
    pub fn card<'a, M: Clone + 'a>(&self, descriptor: Card<'a, M>) -> Element<'a, M> {
        let Card {
            title,
            pages,
            current,
            controls,
            max_height,
            width,
            header_cache,
        } = descriptor;

        let tokens = self.theme.card;
        let width = width.unwrap_or(Length::Fixed(tokens.width));

        let header = container(styled_text(title, tokens.header_style))
            .class(ContainerClass::CardHeader)
            .height(tokens.header_height())
            .width(Length::Fill)
            .padding(tokens.header_padding);
        let header: Element<'a, M> = match header_cache {
            // The default `PixelSnap::LayoutOnly` is what this wants: the
            // header animates nothing itself, but the card's height does, and
            // a card centred in its parent travels as that height
            // interpolates.
            Some(cache) => Cached::new(cache, header).into(),
            None => header.into(),
        };

        let pager: Element<'a, M> = Pager::new(pages)
            .current(current)
            .motion(self.motion.clone())
            .width(Length::Fill)
            .curve(curves::sharp::STRUCTURAL)
            .pixel_snap(PixelSnap::LayoutOnly)
            .into();

        let controls = controls.unwrap_or_else(|| Space::new().into());

        // Two layers, inside out: the body, clipped to the card's rounded
        // rectangle and capped by `max_height`, on the card fill with its
        // tight outline shadow; then the wide halo shadow underneath.
        let mut card = container(Body::new(header, pager, controls))
            .class(ContainerClass::Card)
            .width(width)
            .clip(true);
        if let Some(max_height) = max_height {
            card = card.max_height(max_height);
        }

        container(card)
            .class(ContainerClass::CardHalo)
            .width(width)
            .into()
    }
}

const HEADER: usize = 0;
const PAGER: usize = 1;
const CONTROLS: usize = 2;

/// Header, page stack and controls in a column, measured so the stack gets
/// the height the other two leave.
///
/// iced's `Column` measures shrink children in order and hands each the
/// space the previous ones left, so under a height cap the controls (last)
/// would get nothing (K-010). This widget measures the header and the
/// controls first, then the stack with the remainder.
struct Body<'a, M> {
    children: [Element<'a, M>; 3],
}

impl<'a, M> Body<'a, M> {
    fn new(header: Element<'a, M>, pager: Element<'a, M>, controls: Element<'a, M>) -> Self {
        Self {
            children: [header, pager, controls],
        }
    }
}

impl<M> Widget<M, Theme, Renderer> for Body<'_, M> {
    fn size(&self) -> Size<Length> {
        Size::new(Length::Fill, Length::Shrink)
    }

    fn children(&self) -> Vec<Tree> {
        self.children.iter().map(Tree::new).collect()
    }

    fn diff(&self, tree: &mut Tree) {
        tree.diff_children(&self.children);
    }

    fn layout(&mut self, tree: &mut Tree, renderer: &Renderer, limits: &Limits) -> Node {
        let max = limits.max();
        let unbounded = Limits::new(Size::ZERO, max);

        let header = self.children[HEADER].as_widget_mut().layout(
            &mut tree.children[HEADER],
            renderer,
            &unbounded,
        );
        let controls = self.children[CONTROLS].as_widget_mut().layout(
            &mut tree.children[CONTROLS],
            renderer,
            &unbounded,
        );

        let used = header.size().height + controls.size().height;
        let remaining = (max.height - used).max(0.0);
        let pager = self.children[PAGER].as_widget_mut().layout(
            &mut tree.children[PAGER],
            renderer,
            &Limits::new(Size::ZERO, Size::new(max.width, remaining)),
        );

        let header_height = header.size().height;
        let pager_height = pager.size().height;
        let intrinsic = Size::new(
            header
                .size()
                .width
                .max(pager.size().width)
                .max(controls.size().width),
            header_height + pager_height + controls.size().height,
        );
        let size = limits.resolve(Length::Fill, Length::Shrink, intrinsic);

        Node::with_children(
            size,
            vec![
                header.move_to(Point::ORIGIN),
                pager.move_to(Point::new(0.0, header_height)),
                controls.move_to(Point::new(0.0, header_height + pager_height)),
            ],
        )
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut Renderer,
        theme: &Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        for ((child, tree), layout) in self
            .children
            .iter()
            .zip(&tree.children)
            .zip(layout.children())
        {
            child
                .as_widget()
                .draw(tree, renderer, theme, style, layout, cursor, viewport);
        }
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, M>,
        viewport: &Rectangle,
    ) {
        for ((child, tree), layout) in self
            .children
            .iter_mut()
            .zip(&mut tree.children)
            .zip(layout.children())
        {
            child.as_widget_mut().update(
                tree, event, layout, cursor, renderer, clipboard, shell, viewport,
            );
        }
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &Renderer,
    ) -> mouse::Interaction {
        self.children
            .iter()
            .zip(&tree.children)
            .zip(layout.children())
            .map(|((child, tree), layout)| {
                child
                    .as_widget()
                    .mouse_interaction(tree, layout, cursor, viewport, renderer)
            })
            .max()
            .unwrap_or_default()
    }

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &Renderer,
        operation: &mut dyn Operation,
    ) {
        operation.container(None, layout.bounds());
        operation.traverse(&mut |operation| {
            for ((child, tree), layout) in self
                .children
                .iter_mut()
                .zip(&mut tree.children)
                .zip(layout.children())
            {
                child
                    .as_widget_mut()
                    .operate(tree, layout, renderer, operation);
            }
        });
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout<'b>,
        renderer: &Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, M, Theme, Renderer>> {
        overlay::from_children(
            &mut self.children[..],
            tree,
            layout,
            renderer,
            viewport,
            translation,
        )
    }
}

impl<'a, M: 'a> From<Body<'a, M>> for Element<'a, M> {
    fn from(body: Body<'a, M>) -> Self {
        Self::new(body)
    }
}
