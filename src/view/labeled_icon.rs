use crate::framebuffer::Framebuffer;
use crate::view::{View, Event, Hub, Bus, Align};
use crate::view::icon::Icon;
use crate::view::label::Label;
use crate::geom::Rectangle;
use crate::font::Fonts;
use crate::app::Context;

#[derive(Debug)]
pub struct LabeledIcon {
    rect: Rectangle,
    children: Vec<Box<dyn View>>,
    event: Event,
}

impl LabeledIcon {
    pub fn new(name: &str, rect: Rectangle, event: Event, text: String) -> LabeledIcon {
        let mut children = Vec::new();
        let side = rect.height() as i32;

        let icon = Icon::new(name,
                             rect![rect.min.x, rect.min.y,
                                   rect.min.x + side, rect.max.y],
                             Event::Validate);
        children.push(Box::new(icon) as Box<dyn View>);

        let label = Label::new(rect![rect.min.x + side, rect.min.y,
                                     rect.max.x, rect.max.y],
                               text,
                               Align::Left(0))
                          .event(Some(Event::Validate));
        children.push(Box::new(label) as Box<dyn View>);

        LabeledIcon {
            rect,
            children,
            event,
        }
    }

    pub fn update(&mut self, text: String, hub: &Hub) {
        if let Some(label) = self.children[1].downcast_mut::<Label>() {
            label.update(text, hub);
        }
    }
}

impl View for LabeledIcon {
    fn handle_event(&mut self, evt: &Event, _hub: &Hub, bus: &mut Bus, _context: &mut Context) -> bool {
        match *evt {
            Event::Validate => {
                if let Event::Show(view_id) = self.event {
                    bus.push_back(Event::ToggleNear(view_id, self.rect));
                } else {
                    bus.push_back(self.event.clone());
                }
                true
            },
            _ => false,
        }
    }

    fn render(&self, _fb: &mut dyn Framebuffer, _rect: Rectangle, _fonts: &mut Fonts) -> Rectangle {
        self.rect
    }

    fn resize(&mut self, rect: Rectangle, hub: &Hub, context: &mut Context) {
        let side = rect.height() as i32;
        self.children[0].resize(rect![rect.min.x, rect.min.y,
                                      rect.min.x + side, rect.max.y],
                                hub, context);
        self.children[1].resize(rect![rect.min.x + side, rect.min.y,
                                     rect.max.x, rect.max.y],
                                hub, context);
        if let Event::ToggleNear(_, ref mut event_rect) = self.event {
            *event_rect = rect;
        }
        self.rect = rect;
    }

    fn rect(&self) -> &Rectangle {
        &self.rect
    }

    fn rect_mut(&mut self) -> &mut Rectangle {
        &mut self.rect
    }

    fn children(&self) -> &Vec<Box<dyn View>> {
        &self.children
    }

    fn children_mut(&mut self) -> &mut Vec<Box<dyn View>> {
        &mut self.children
    }
}
