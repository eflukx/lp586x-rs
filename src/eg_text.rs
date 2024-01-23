extern crate alloc;
use alloc::boxed::Box;

use embedded_graphics::{
    mono_font::{self, MonoFont, MonoTextStyle},
    text::{renderer::TextRenderer, Text},
};
use embedded_graphics_core::{pixelcolor::Gray8, prelude::*};
use embedded_hal::digital::v2::OutputPin;

use crate::{egfx::Lp586xDisplay1x2, PwmAccess};

pub trait DrawHText {
    type Error;
    type Ouput;

    const DEFAULT_FONT: MonoFont<'static> = mono_font::ascii::FONT_5X8;
    const DEFAULT_TEXT_FRONT_STYLE: MonoTextStyle<'static, Gray8> =
        MonoTextStyle::new(&Self::DEFAULT_FONT, Gray8::WHITE);
    const DEFAULT_TEXT_WIPE_STYLE: MonoTextStyle<'static, Gray8> =
        MonoTextStyle::new(&Self::DEFAULT_FONT, Gray8::BLACK);

    fn draw_scrolltext_frame(
        &mut self,
        // text: &str,
        text_drawable: &mut Text<'_, MonoTextStyle<'_, Gray8>>,
        x_offset: i32,
    ) -> Result<Option<Self::Ouput>, Self::Error>;

    fn initial_x_offset(&self) -> i32;

    fn default_y_offset(&self) -> i32 {
        // maybe fix this to 6?
        Self::DEFAULT_TEXT_FRONT_STYLE.line_height() as i32 - 2
    }

    fn scroll_x_offsets_iter(
        &self,
        drawable: impl Dimensions,
    ) -> impl Iterator<Item = i32> + 'static;
}

impl<D, VP> DrawHText for Lp586xDisplay1x2<D, VP>
where
    D: PwmAccess<u8> + OriginDimensions,
    VP: OutputPin,
{
    type Error = D::Error;
    type Ouput = Point;

    fn draw_scrolltext_frame(
        &mut self,
        // text: &str,
        text_drawable: &mut Text<'_, MonoTextStyle<'_, Gray8>>,
        x_offset: i32,
    ) -> Result<Option<Point>, D::Error> {
        // let mut t = Text::new(text, position, Self::DEFAULT_TEXT_FRONT_STYLE);
        text_drawable.character_style = Self::DEFAULT_TEXT_FRONT_STYLE;
        text_drawable.position = Point::new(x_offset, self.default_y_offset());
        text_drawable.draw(self)?;

        self.toggle_sync(); // Draw!

        text_drawable.character_style = Self::DEFAULT_TEXT_WIPE_STYLE;
        text_drawable.draw(self).map(Option::Some)
    }

    fn scroll_x_offsets_iter(
        &self,
        drawable: impl Dimensions,
    ) -> impl Iterator<Item = i32> + 'static {
        let drawable_width = drawable.bounding_box().size.width as i32;
        (-drawable_width..=self.initial_x_offset()).rev().clone()
    }

    fn initial_x_offset(&self) -> i32 {
        self.size().width as i32
    }
}

pub struct TextScroller<'a, D> {
    display: &'a mut D,
    x_iter: Box<dyn Iterator<Item = i32>>,
    text_drawable: Text<'a, MonoTextStyle<'a, Gray8>>,
}

impl<'a, D> TextScroller<'a, D>
where
    D: DrawHText,
{
    pub fn new(display: &'a mut D, text: &'a str) -> Self {
        let text_drawable = Text::new(text, Point::zero(), D::DEFAULT_TEXT_FRONT_STYLE);
        let x_iter = display.scroll_x_offsets_iter(text_drawable);

        Self {
            display,
            x_iter: Box::new(x_iter),
            text_drawable,
        }
    }
}

impl<'a, D> Iterator for TextScroller<'a, D>
where
    D: DrawHText,
{
    type Item = i32;

    fn next(&mut self) -> Option<Self::Item> {
        let x_offset = self.x_iter.next()?;
        self.display
            .draw_scrolltext_frame(&mut self.text_drawable, x_offset)
            .ok()
            .unwrap();

        Some(x_offset)
    }
}
