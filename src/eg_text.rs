extern crate alloc;
use core::marker::PhantomData;

use alloc::boxed::Box;

use embedded_graphics::{
    mono_font::{self, MonoFont, MonoTextStyle},
    text::{renderer::TextRenderer, Text},
};
use embedded_graphics_core::{pixelcolor::Gray8, prelude::*};
use embedded_hal::digital::v2::OutputPin;

use crate::{egfx::Lp586xDisplay1x2, PwmAccess};

pub trait HScroll {
    type Error;
    type Output;

    const DEFAULT_FONT: MonoFont<'static> = mono_font::ascii::FONT_5X8;
    const DEFAULT_TEXT_FRONT_STYLE: MonoTextStyle<'static, Gray8> =
        MonoTextStyle::new(&Self::DEFAULT_FONT, Gray8::WHITE);
    const DEFAULT_TEXT_WIPE_STYLE: MonoTextStyle<'static, Gray8> =
        MonoTextStyle::new(&Self::DEFAULT_FONT, Gray8::BLACK);

    fn draw_scrolltext_frame(
        &mut self,
        // text: &str,
        text_drawable: &mut Text<'_, MonoTextStyle<'_, Gray8>>,
        position: Point,
    ) -> Result<Option<Self::Output>, Self::Error>;

    fn initial_x_offset(&self) -> i32;

    // At which position do we start scrollin'?
    fn default_offset(&self) -> Point {
        Point::new(0, Self::DEFAULT_TEXT_FRONT_STYLE.line_height() as i32 - 2)
    }

    fn h_scroll_position_iter(
        &self,
        drawable: impl Dimensions,
    ) -> impl Iterator<Item = Point> + 'static;
}

impl<D, VP> HScroll for Lp586xDisplay1x2<D, VP>
where
    D: PwmAccess<u8> + OriginDimensions,
    VP: OutputPin,
{
    type Error = D::Error;
    type Output = Point;

    fn draw_scrolltext_frame(
        &mut self,
        text_drawable: &mut Text<'_, MonoTextStyle<'_, Gray8>>,
        position: Point,
    ) -> Result<Option<Point>, D::Error> {
        text_drawable.character_style = Self::DEFAULT_TEXT_FRONT_STYLE;
        text_drawable.position = position + self.default_offset();
        text_drawable.draw(self)?;

        self.toggle_sync(); // Show!

        text_drawable.character_style = Self::DEFAULT_TEXT_WIPE_STYLE;
        text_drawable.draw(self).map(Option::Some)
    }

    fn h_scroll_position_iter(
        &self,
        drawable: impl Dimensions,
    ) -> impl Iterator<Item = Point> + 'static {
        let drawable_width = drawable.bounding_box().size.width as i32;
        (-drawable_width..=self.initial_x_offset())
            .rev()
            .map(|x_off| Point::new(x_off, 0))
            .clone()
    }

    fn initial_x_offset(&self) -> i32 {
        self.size().width as i32
    }
}

pub struct ScrollIter<'a, D> {
    display: PhantomData<D>,
    position_iter: Box<dyn Iterator<Item = Point>>,
    text_drawable: Text<'a, MonoTextStyle<'a, Gray8>>,
}

impl<'a, D> ScrollIter<'a, D>
where
    D: HScroll,
{
    pub fn new(display: &D, text: &'a str) -> Self {
        let text_drawable = Text::new(text, Point::zero(), D::DEFAULT_TEXT_FRONT_STYLE);
        let x_iter = display.h_scroll_position_iter(text_drawable);

        Self {
            display: PhantomData,
            position_iter: Box::new(x_iter),
            text_drawable,
        }
    }

    pub fn draw_next_frame(&mut self, display: &mut D) -> Result<Option<Point>, D::Error> {
        match self.next() {
            Some(offset) => {
                display.draw_scrolltext_frame(&mut self.text_drawable, offset)?;
                Ok(Some(offset))
            }
            None => Ok(None),
        }
    }
}

impl<'a, D> Iterator for ScrollIter<'a, D>
where
    D: HScroll,
{
    type Item = Point;

    fn next(&mut self) -> Option<Self::Item> {
        self.position_iter.next()
    }
}
