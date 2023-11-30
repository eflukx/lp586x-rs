use crate::{
    configuration::Configuration, interface::RegisterAccess, DataMode8Bit, DataModeMarker,
    DeviceVariant, Lp586x, PwmAccess,
};
pub use embedded_graphics as eg;
use embedded_graphics::{pixelcolor::Gray8, prelude::*};
use embedded_hal::digital::v2::OutputPin;

/// Simple composited display of two LP586x controllers
/// stacked in height i.e. '1x2'
pub struct Lp586xDisplay1x2<U, L, VP> {
    upper: U,
    lower: L,
    vsync_pin: VP,
}

impl<VP, DV, I, DM, IE> Lp586xDisplay1x2<Lp586x<DV, I, DM>, Lp586x<DV, I, DM>, VP>
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

impl<U, L, VP> Lp586xDisplay1x2<U, L, VP> {
    pub fn upper_mut(&mut self) -> &mut U {
        &mut self.upper
    }

    pub fn lower_mut(&mut self) -> &mut L {
        &mut self.lower
    }
}

impl<U, L, VP> Lp586xDisplay1x2<U, L, VP>
where
    U: PwmAccess<u8> + OriginDimensions,
    L: PwmAccess<u8> + OriginDimensions,
    VP: OutputPin,
{
    pub fn new(upper: U, lower: L, vsync_pin: VP) -> Result<Self, crate::Error<()>> {
        if upper.size() == lower.size() {
            Ok(Lp586xDisplay1x2 {
                upper,
                lower,
                vsync_pin,
            })
        } else {
            Err(crate::Error::SizeMismatch)
        }
    }

    /// Immediately draw a single pixel.
    /// Drawing this way (per pixel) certainly is not too efficient
    pub fn draw_pixel(
        &mut self,
        Pixel(point, color): Pixel<impl GrayColor>,
    ) -> Result<(), U::Error> {
        let luma = color.luma();

        match self.controller_idx_and_offset(point) {
            Some((0, offset)) => self.upper.set_pwm(offset, &[luma]),
            Some((1, offset)) => self.lower.set_pwm(offset, &[luma]),
            _ => Ok(()),
        }
    }

    /// returns the controller and dot (offset) for a given point
    /// return None is the `Point` is out of bounds
    fn controller_idx_and_offset(&self, point: Point) -> Option<(u16, u16)> {
        // defmt::warn!("zelf.bboxdraw {}, point{},{}", defmt::Debug2Format(&self.bounding_box()),point.x,point.y);

        let point_fl = Point::new(self.size().width as i32 - point.x, point.y);
        self.bounding_box().contains(point_fl).then(|| {
            if self.upper.bounding_box().contains(point_fl) {
                let offset = point_fl.y * self.size().width as i32 + point_fl.x;
                (0, offset as u16)
            } else {
                // subtract the height of the upper part to get correct offset for lower controller
                let offset = (point_fl.y - self.upper.size().height as i32)
                    * self.size().width as i32
                    + point_fl.x;
                (1, offset as u16)
            }
        })
    }

    pub fn toggle_sync(&mut self) {
        self.vsync_pin.set_high();
        self.vsync_pin.set_high();
        self.vsync_pin.set_high();
        self.vsync_pin.set_low();
    }
}

impl<U, L, VP> OriginDimensions for Lp586xDisplay1x2<U, L, VP>
where
    U: OriginDimensions,
{
    /// We use the dimension of the upper display only, as we assume both displays are the same
    /// probably should enforce this in the type of `Lp586xDisplay1x2<U, L, VP>`, making `U` and `L` one
    fn size(&self) -> Size {
        Size {
            width: self.upper.size().width,
            height: self.upper.size().height * 2,
        }
    }
}

impl<U, L, VP> DrawTarget for Lp586xDisplay1x2<U, L, VP>
where
    U: PwmAccess<u8> + OriginDimensions,
    L: PwmAccess<u8> + OriginDimensions,
    VP: OutputPin,
{
    type Color = Gray8;
    type Error = U::Error; // Hmm how to handle the two "different" errors (which we know are the same type) neatly?

    fn draw_iter<I>(&mut self, pixels: I) -> Result<(), Self::Error>
    where
        I: IntoIterator<Item = Pixel<Self::Color>>,
    {
        for px in pixels {
            self.draw_pixel(px)?;
        }
        // self.vsync_pin.set_high();
        Ok(())
    }
}

impl<DV: DeviceVariant, I: RegisterAccess<Error = IfErr>, IfErr> DrawTarget
    for Lp586x<DV, I, DataMode8Bit>
{
    type Color = Gray8;
    type Error = crate::Error<I::Error>;

    fn draw_iter<C>(&mut self, pixels: C) -> Result<(), Self::Error>
    where
        C: IntoIterator<Item = Pixel<Self::Color>>,
    {
        let _linez = self.num_lines();
        self.set_pwm(0, &[]);
        // let mut buf = [Gray8::BLACK; Self::NUM_DOTS];

        let _px = pixels.into_iter().next().unwrap();
        // px.
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
