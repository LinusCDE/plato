use device::CURRENT_DEVICE;
use unit::scale_by_dpi;
use framebuffer::{Framebuffer, UpdateMode};
use input::{DeviceEvent, FingerStatus};
use view::{View, Event, Hub, Bus, SliderId, THICKNESS_SMALL};
use color::{BLACK, WHITE, PROGRESS_VALUE, PROGRESS_FULL, PROGRESS_EMPTY};
use font::{Fonts, font_from_style, SLIDER_VALUE};
use geom::{Rectangle, BorderSpec, CornerSpec, halves};
use app::Context;

const PROGRESS_HEIGHT: f32 = 7.0;
const BUTTON_DIAMETER: f32 = 46.0;

pub struct Slider {
    pub rect: Rectangle,
    children: Vec<Box<View>>,
    id: SliderId,
    value: f32,
    min_value: f32,
    max_value: f32,
    active: bool,
    last_x: i32,
}

impl Slider {
    pub fn new(rect: Rectangle, id: SliderId, value: f32, min_value: f32, max_value: f32) -> Slider {
        Slider {
            rect,
            children: vec![],
            id,
            value,
            min_value,
            max_value,
            active: false,
            last_x: -1,
        }
    }

    pub fn update_value(&mut self, x_hit: i32) {
        let dpi = CURRENT_DEVICE.dpi;
        let button_diameter = scale_by_dpi(BUTTON_DIAMETER, dpi) as i32;
        let (small_radius, big_radius) = halves(button_diameter);
        let x_offset = x_hit.max(self.rect.min.x + small_radius)
                            .min(self.rect.max.x - big_radius);
        let progress = ((x_offset - self.rect.min.x - small_radius) as f32 /
                        (self.rect.width() as i32 - button_diameter) as f32)
                       .min(1.0).max(0.0);
        self.value = self.min_value + progress * (self.max_value - self.min_value);
    }
}

impl View for Slider {
    fn handle_event(&mut self, evt: &Event, hub: &Hub, bus: &mut Bus, _context: &mut Context) -> bool {
        match *evt {
            Event::Device(DeviceEvent::Finger { status, ref position, .. }) => {
                match status {
                    FingerStatus::Down if self.rect.includes(position) => {
                        self.active = true;
                        self.update_value(position.x);
                        hub.send(Event::Render(self.rect, UpdateMode::Gui)).unwrap();
                        bus.push_back(Event::Slider(self.id, self.value, status));
                        self.last_x = position.x;
                        true
                    },
                    FingerStatus::Motion if self.active && position.x != self.last_x => {
                        self.update_value(position.x);
                        hub.send(Event::RenderNoWait(self.rect, UpdateMode::FastMono)).unwrap();
                        bus.push_back(Event::Slider(self.id, self.value, status));
                        self.last_x = position.x;
                        true
                    },
                    FingerStatus::Up if self.active => {
                        self.active = false;
                        if position.x != self.last_x {
                            self.update_value(position.x);
                            self.last_x = position.x;
                        }
                        hub.send(Event::Render(self.rect, UpdateMode::Gui)).unwrap();
                        bus.push_back(Event::Slider(self.id, self.value, status));
                        true
                    },
                    _ => self.active,
                }
            },
            _ => false,
        }
    }

    fn render(&self, fb: &mut Framebuffer, fonts: &mut Fonts) {
        let dpi = CURRENT_DEVICE.dpi;
        let progress_height = scale_by_dpi(PROGRESS_HEIGHT, dpi) as i32;
        let button_diameter = scale_by_dpi(BUTTON_DIAMETER, dpi) as i32;
        let border_thickness = scale_by_dpi(THICKNESS_SMALL, dpi) as u16;

        let progress = (self.value - self.min_value) / (self.max_value - self.min_value);
        let (small_radius, big_radius) = halves(button_diameter);
        let x_offset = self.rect.min.x + small_radius +
                       ((self.rect.width() as f32 - button_diameter as f32) * progress) as i32;

        fb.draw_rectangle(&self.rect, WHITE);

        let (small_mini_radius, big_mini_radius) = halves(progress_height);
        let (small_padding, big_padding) = halves(self.rect.height() as i32 - progress_height);
        let rect = rect![self.rect.min.x + small_radius - big_mini_radius, self.rect.min.y + small_padding,
                         self.rect.max.x - big_radius + small_mini_radius, self.rect.max.y - big_padding];

        fb.draw_rounded_rectangle_with_border(&rect,
                                              &CornerSpec::Uniform(small_mini_radius),
                                              &BorderSpec { thickness: border_thickness,
                                                            color: BLACK },
                                              &|x, _| if x < x_offset { PROGRESS_FULL }
                                                      else { PROGRESS_EMPTY });

        let (small_padding, big_padding) = halves(self.rect.height() as i32 - button_diameter);
        let rect = rect![x_offset - small_radius, self.rect.min.y + small_padding,
                         x_offset + big_radius, self.rect.max.y - big_padding];
        let fill_color = if self.active { BLACK } else { WHITE };

        fb.draw_rounded_rectangle_with_border(&rect,
                                              &CornerSpec::Uniform(small_radius),
                                              &BorderSpec { thickness: 2 * border_thickness,
                                                            color: BLACK },
                                              &fill_color);

        let font = font_from_style(fonts, &SLIDER_VALUE, dpi);
        let plan = font.plan(&format!("{:.1}", self.value), None, None);
        let x_height = font.x_heights.1 as i32;

        let x_drift = if self.value > (self.min_value + self.max_value) / 2.0 {
            -(small_radius + plan.width as i32)
        } else {
            small_radius
        };

        let pt = pt!(x_offset + x_drift, self.rect.min.y + x_height.max(small_padding));
        font.render(fb, PROGRESS_VALUE, &plan, &pt);
    }

    fn rect(&self) -> &Rectangle {
        &self.rect
    }

    fn rect_mut(&mut self) -> &mut Rectangle {
        &mut self.rect
    }

    fn children(&self) -> &Vec<Box<View>> {
        &self.children
    }

    fn children_mut(&mut self) -> &mut Vec<Box<View>> {
        &mut self.children
    }
}
