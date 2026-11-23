//! Number fields. On a desktop this is egui's DragValue (drag to change, click to type).
//! On a touch screen dragging a number fights scrolling, so there it is a box you tap
//! to type a new value into, and a swipe across it scrolls the page.

use eframe::egui::{self, emath::Numeric, Response, Ui, Widget};
use std::ops::RangeInclusive;

pub struct Num<'a, N: Numeric> {
    value: &'a mut N,
    speed: f64,
    range: RangeInclusive<f64>,
    prefix: String,
    suffix: String,
    min_decimals: usize,
    max_decimals: Option<usize>,
}

impl<'a, N: Numeric> Num<'a, N> {
    pub fn new(value: &'a mut N) -> Self {
        Num { value, speed: 1.0, range: f64::NEG_INFINITY..=f64::INFINITY, prefix: String::new(), suffix: String::new(), min_decimals: 0, max_decimals: None }
    }
    pub fn speed(mut self, s: impl Into<f64>) -> Self {
        self.speed = s.into();
        self
    }
    pub fn range<R: Numeric>(mut self, r: RangeInclusive<R>) -> Self {
        self.range = r.start().to_f64()..=r.end().to_f64();
        self
    }
    pub fn prefix(mut self, p: impl ToString) -> Self {
        self.prefix = p.to_string();
        self
    }
    pub fn suffix(mut self, s: impl ToString) -> Self {
        self.suffix = s.to_string();
        self
    }
    pub fn min_decimals(mut self, d: usize) -> Self {
        self.min_decimals = d;
        self
    }
    pub fn max_decimals(mut self, d: usize) -> Self {
        self.max_decimals = Some(d);
        self
    }
}

impl<N: Numeric> Widget for Num<'_, N> {
    fn ui(self, ui: &mut Ui) -> Response {
        if !crate::theme::is_touch() {
            let mut d = egui::DragValue::new(self.value).speed(self.speed).prefix(self.prefix).suffix(self.suffix).min_decimals(self.min_decimals);
            if self.range.start().is_finite() || self.range.end().is_finite() {
                d = d.range(self.range);
            }
            if let Some(m) = self.max_decimals {
                d = d.max_decimals(m);
            }
            return ui.add(d);
        }
        touch_field(self, ui)
    }
}

/// Decimals as DragValue shows them: enough for the drag speed, within min..=max.
fn decimals(ui: &Ui, speed: f64, min: usize, max: Option<usize>, integral: bool) -> (usize, usize) {
    if integral {
        return (0, 0);
    }
    let aim = ui.input(|i| i.aim_radius()) as f64;
    let auto = (aim / speed.abs().max(1e-12)).log10().ceil().clamp(0.0, 15.0) as usize;
    let max = max.unwrap_or(auto + 2).max(min);
    (auto.clamp(min, max), max)
}

fn touch_field<N: Numeric>(n: Num<'_, N>, ui: &mut Ui) -> Response {
    let id = ui.next_auto_id();
    ui.skip_ahead_auto_ids(1);
    let old = n.value.to_f64();
    let (lo, hi) = decimals(ui, n.speed, n.min_decimals, n.max_decimals, N::INTEGRAL);
    let shown = egui::emath::format_with_decimals_in_range(old, lo..=hi);
    let editing: Option<String> = ui.data(|d| d.get_temp(id));
    let mut resp = match editing {
        Some(mut text) => {
            let r = ui.add(
                egui::TextEdit::singleline(&mut text)
                    .id(id)
                    .desired_width(ui.spacing().interact_size.x.max(70.0))
                    .horizontal_align(egui::Align::Center),
            );
            if !r.has_focus() && !r.lost_focus() {
                r.request_focus();
            }
            if r.lost_focus() {
                ui.data_mut(|d| d.remove::<String>(id));
                let t = text.trim().trim_start_matches(n.prefix.trim()).trim_end_matches(n.suffix.trim()).trim().replace(',', ".");
                if let Ok(v) = t.parse::<f64>() {
                    let v = v.clamp(*n.range.start(), *n.range.end());
                    *n.value = N::from_f64(if N::INTEGRAL { v.round() } else { v });
                }
            } else {
                ui.data_mut(|d| d.insert_temp(id, text));
            }
            r
        }
        None => {
            let r = ui.add(egui::Button::new(format!("{}{}{}", n.prefix, shown, n.suffix)).min_size(ui.spacing().interact_size));
            if r.clicked() {
                ui.data_mut(|d| d.insert_temp(id, shown.clone()));
                ui.memory_mut(|m| m.request_focus(id));
                // select it all, so typing replaces the value
                let mut st = egui::text_edit::TextEditState::load(ui.ctx(), id).unwrap_or_default();
                let all = egui::text::CCursorRange::two(egui::text::CCursor::new(0), egui::text::CCursor::new(shown.chars().count()));
                st.cursor.set_char_range(Some(all));
                st.store(ui.ctx(), id);
            }
            r
        }
    };
    if n.value.to_f64() != old {
        resp.mark_changed();
    }
    resp
}
