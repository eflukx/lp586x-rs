use core::ops::RangeInclusive;

use crate::{
    configuration::Configuration, interface::RegisterAccess, DataModeMarker, DeviceVariant, Lp586x,
    PwmAccess,
};
use eg::{pixelcolor::Gray8, prelude::*};
use embedded_graphics::{
    mono_font::{self, MonoFont, MonoTextStyle},
    text::renderer::TextRenderer,
};
pub use embedded_graphics_core as eg;
use embedded_hal::digital::v2::OutputPin;

/// Simple composited display of two LP586x controllers
/// stacked in height i.e. '1x2'
pub struct Lp586xDisplay1x2<D, VP> {
    upper: D,
    lower: D,
    vsync_pin: VP,
}

impl<VP, DV, I, DM> Lp586xDisplay1x2<Lp586x<DV, I, DM>, VP>
where
    DV: DeviceVariant,
{
    pub const WIDTH: u32 = DV::NUM_CURRENT_SINKS as u32;
    pub const HEIGHT: u32 = DV::NUM_LINES as u32 * 2;
    pub const SIZE: Size = Size {
        width: Self::WIDTH,
        height: Self::HEIGHT,
    };
}

impl<VP, DV, I, DM, IE> Lp586xDisplay1x2<Lp586x<DV, I, DM>, VP>
where
    I: RegisterAccess<Error = crate::Error<IE>>,
    DV: DeviceVariant,
    DM: DataModeMarker,
{
    pub fn set_global_brightness(&mut self, brightness: u8) -> Result<(), crate::Error<IE>> {
        self.upper_mut().set_global_brightness(brightness)?;
        self.lower_mut().set_global_brightness(brightness)
    }

    pub fn configure(&mut self, configuration: &Configuration) -> Result<(), crate::Error<IE>> {
        self.upper_mut().configure(configuration)?;
        self.lower_mut().configure(configuration)
    }

    pub fn enable(&mut self, enable: bool) -> Result<(), crate::Error<IE>> {
        self.upper_mut().chip_enable(enable)?;
        self.lower_mut().chip_enable(enable)
    }
}

impl<D, VP> Lp586xDisplay1x2<D, VP> {
    pub fn into_parts(self) -> (D, D, VP) {
        (self.upper, self.lower, self.vsync_pin)
    }

    pub fn upper_mut(&mut self) -> &mut D {
        &mut self.upper
    }

    pub fn lower_mut(&mut self) -> &mut D {
        &mut self.lower
    }
}

impl<D, VP> Lp586xDisplay1x2<D, VP>
where
    D: PwmAccess<u8> + OriginDimensions,
    VP: OutputPin,
{
    pub fn new(upper: D, lower: D, vsync_pin: VP) -> Self {
        Lp586xDisplay1x2 {
            upper,
            lower,
            vsync_pin,
        }
    }

    /// Immediately draw a single pixel.
    /// Drawing this way (per pixel) certainly is not too efficient
    pub fn draw_pixel(
        &mut self,
        Pixel(point, color): Pixel<impl GrayColor>,
    ) -> Result<(), D::Error> {
        let luma = color.luma();

        match self.controller_idx_and_offset(point) {
            Some((0, offset)) => self.upper.set_pwm(offset, &[luma]),
            Some((1, offset)) => self.lower.set_pwm(offset, &[luma]),
            _ => Ok(()),
        }
    }

    /// returns the controller and dot (offset) for a given `Point`
    /// return `None` is the `Point` is out of bounds
    fn controller_idx_and_offset(&self, point: Point) -> Option<(u16, u16)> {
        // H-Flip point
        let point = Point::new(self.size().width as i32 - point.x - 1, point.y);

        self.bounding_box().contains(point).then(|| {
            if self.upper.bounding_box().contains(point) {
                let offset = point.y * self.size().width as i32 + point.x;
                (0, offset as u16)
            } else {
                // subtract the height of the upper part to get correct offset for lower controller
                let offset = (point.y - self.upper.size().height as i32) * self.size().width as i32
                    + point.x;
                (1, offset as u16)
            }
        })
    }

    pub fn toggle_sync(&mut self) {
        for _ in 1..15 {
            // dirty.. but works for now (making high pulse wide enough)..
            let _ = self.vsync_pin.set_high();
        }
        let _ = self.vsync_pin.set_low();
    }
}

impl<D, VP> OriginDimensions for Lp586xDisplay1x2<D, VP>
where
    D: OriginDimensions,
{
    fn size(&self) -> Size {
        Size {
            width: self.upper.size().width,
            height: self.upper.size().height * 2,
        }
    }
}

impl<D, VP> DrawTarget for Lp586xDisplay1x2<D, VP>
where
    D: PwmAccess<u8> + OriginDimensions,
    VP: OutputPin,
{
    type Color = Gray8; // how to implement this for all types implementing GrayColor?
    type Error = D::Error; // Hmm how to handle the two "different" errors (which we know are the same type) neatly?

    fn draw_iter<I>(&mut self, pixels: I) -> Result<(), Self::Error>
    where
        I: IntoIterator<Item = Pixel<Self::Color>>,
    {
        for px in pixels {
            self.draw_pixel(px)?;
        }

        Ok(())
    }
}

impl<DV: DeviceVariant, I, DM> OriginDimensions for Lp586x<DV, I, DM> {
    fn size(&self) -> Size {
        // TODO: support more that jsut the "full frame"
        // e.g. by using something like `self.active_lines()`
        Size::new(DV::NUM_CURRENT_SINKS as u32, DV::NUM_LINES as u32)
    }
}
