//! Driver for the Texas Instruments LP586x LED matrix driver. Supports the LP5860,
//! LP5861, LP5862, LP5864 and LP5868 subvariants.
//!
//! Datasheet: <https://www.ti.com/lit/ds/symlink/lp5864.pdf>
//!
//! Register map: <https://www.ti.com/lit/ug/snvu786/snvu786.pdf>

#![cfg_attr(not(test), no_std)]

pub mod configuration;
pub mod interface;
mod register;

use configuration::Configuration;
use interface::{RegisterAccess, SpiInterfaceError};
use register::{BitFlags, Register};

/// Error enum for the LP586x driver
#[derive(Debug)]
pub enum Error<IE> {
    /// An interface related error has occured
    Interface(IE),

    /// Temporary buffer too small
    BufferOverrun,
}

/// Output PWM frequency setting
#[derive(Debug)]
pub enum PwmFrequency {
    /// 125 kHz
    Pwm125kHz,
    /// 62.5 kHz
    Pwm62_5kHz,
}

/// Line switch blanking time setting
#[derive(Debug)]
pub enum LineBlankingTime {
    /// 1µs
    Blank1us,
    /// 0.5µs
    Blank0_5us,
}

/// Dimming scale setting of final PWM generator
#[derive(Debug)]
pub enum PwmScaleMode {
    /// Linear scale dimming curve
    Linear,
    /// Exponential scale dimming curve
    Exponential,
}

/// Downside deghosting level selection
#[derive(Debug)]
pub enum DownDeghost {
    None,
    Weak,
    Medium,
    Strong,
}

impl DownDeghost {
    pub const fn register_value(&self) -> u8 {
        match self {
            DownDeghost::None => 0,
            DownDeghost::Weak => 1,
            DownDeghost::Medium => 2,
            DownDeghost::Strong => 3,
        }
    }
}

/// Scan line clamp voltage of upside deghosting
#[derive(Debug)]
pub enum UpDeghost {
    /// VLED - 2V
    VledMinus2V,
    /// VLED - 2.5V
    VledMinus2_5V,
    /// VLED - 3V
    VledMinus3V,
    /// GND
    Gnd,
}

impl UpDeghost {
    pub const fn register_value(&self) -> u8 {
        match self {
            UpDeghost::VledMinus2V => 0,
            UpDeghost::VledMinus2_5V => 1,
            UpDeghost::VledMinus3V => 2,
            UpDeghost::Gnd => 3,
        }
    }
}

/// Data refresh mode selection
#[derive(Debug)]
pub enum DataRefMode {
    /// 8 bit PWM, update instantly, no external VSYNC
    Mode1,
    /// 8 bit PWM, update by frame, external VSYNC
    Mode2,
    /// 16 bit PWM, update by frame, external VSYNC
    Mode3,
}

impl DataRefMode {
    pub const fn register_value(&self) -> u8 {
        match self {
            DataRefMode::Mode1 => 0,
            DataRefMode::Mode2 => 1,
            DataRefMode::Mode3 => 2,
        }
    }
}

/// Maximum current cetting
#[derive(Debug)]
pub enum CurrentSetting {
    Max3mA,
    Max5mA,
    Max10mA,
    Max15mA,
    Max20mA,
    Max30mA,
    Max40mA,
    Max50mA,
}

impl CurrentSetting {
    pub const fn register_value(&self) -> u8 {
        match self {
            CurrentSetting::Max3mA => 0,
            CurrentSetting::Max5mA => 1,
            CurrentSetting::Max10mA => 2,
            CurrentSetting::Max15mA => 3,
            CurrentSetting::Max20mA => 4,
            CurrentSetting::Max30mA => 5,
            CurrentSetting::Max40mA => 6,
            CurrentSetting::Max50mA => 7,
        }
    }
}

/// Fixed color groups for current sinks
#[derive(Debug)]
pub enum Group {
    /// CS0, CS3, CS6, CS9, CS12, CS15
    Group0,
    /// CS1, CS4, CS7, CS10, CS13, CS16
    Group1,
    /// CS2, CS5, CS8, CS11, CS14, CS17
    Group2,
}

impl Group {
    pub fn brightness_reg_addr(&self) -> u16 {
        match self {
            Group::Group0 => Register::GROUP0_BRIGHTNESS,
            Group::Group1 => Register::GROUP1_BRIGHTNESS,
            Group::Group2 => Register::GROUP2_BRIGHTNESS,
        }
    }

    pub fn current_reg_addr(&self) -> u16 {
        match self {
            Group::Group0 => Register::GROUP0_CURRENT,
            Group::Group1 => Register::GROUP1_CURRENT,
            Group::Group2 => Register::GROUP2_CURRENT,
        }
    }
}

/// Configurable group for each dot
#[derive(Debug, Clone, Copy)]
pub enum DotGroup {
    None,
    Group0,
    Group1,
    Group2,
}

impl DotGroup {
    fn register_value(&self) -> u8 {
        match self {
            DotGroup::None => 0,
            DotGroup::Group0 => 0b01,
            DotGroup::Group1 => 0b10,
            DotGroup::Group2 => 0b11,
        }
    }
}

#[derive(Debug)]
pub struct GlobalFaultState {
    led_open_detected: bool,
    led_short_detected: bool,
}

impl GlobalFaultState {
    pub fn from_reg_value(fault_state_value: u8) -> Self {
        GlobalFaultState {
            led_open_detected: fault_state_value & BitFlags::FAULT_STATE_GLOBAL_LOD > 0,
            led_short_detected: fault_state_value & BitFlags::FAULT_STATE_GLOBAL_LSD > 0,
        }
    }

    /// True, if any LED is detected open.
    ///
    /// LED open detection is only performed when PWM ≥ 25 (Mode 1 and Mode 2) or
    /// PWM ≥ 6400 (Mode 3) and voltage on CSn is detected lower than open threshold
    /// for continuously 4 sub-periods.
    pub fn led_open_detected(&self) -> bool {
        self.led_open_detected
    }

    /// True, if any LED is detected shorted.
    ///
    /// LED short detection only performed when PWM ≥ 25 (Mode 1 and Mode 2) or
    /// PWM ≥ 6400 (Mode 3) and voltage on CSn is detected higher than short threshold
    // for continuously 4 sub-periods.
    pub fn led_short_detected(&self) -> bool {
        self.led_short_detected
    }
}

/// Represents a safe way to address a dot in the matrix.
pub struct Dot<DV>(u16, core::marker::PhantomData<DV>);

impl<DV: DeviceVariant> Dot<DV> {
    /// Create [`Dot`] at `index`. Panics if given `index` is outside the device
    /// variants capabilites.
    pub fn with_index(index: u16) -> Self {
        if index / 18 > DV::NUM_LINES as u16 {
            panic!("Device variant does not support dot {index}");
        }

        Self(index, core::marker::PhantomData)
    }

    pub fn index(&self) -> u16 {
        self.0
    }

    pub fn line(&self) -> u16 {
        self.0 / 18
    }

    pub fn current_sink(&self) -> u16 {
        self.0 % 18
    }
}

mod seal {
    pub trait Sealed {}
}

/// Marker trait for a device variant.
pub trait DeviceVariant: seal::Sealed {
    /// Number of scan lines of this device variant.
    const NUM_LINES: u8;

    /// Number of current sinks of this device variant.
    const NUM_CURRENT_SINKS: u8 = 18;

    /// Total number of LED dots of this device variant.
    const NUM_DOTS: u16 = Self::NUM_LINES as u16 * Self::NUM_CURRENT_SINKS as u16;
}

#[doc(hidden)]
pub struct Variant0;
impl DeviceVariant for Variant0 {
    const NUM_LINES: u8 = 11;
}
impl seal::Sealed for Variant0 {}

#[doc(hidden)]
pub struct Variant1;
impl DeviceVariant for Variant1 {
    const NUM_LINES: u8 = 1;
}
impl seal::Sealed for Variant1 {}

#[doc(hidden)]
pub struct Variant2;
impl DeviceVariant for Variant2 {
    const NUM_LINES: u8 = 2;
}
impl seal::Sealed for Variant2 {}

#[doc(hidden)]
pub struct Variant4;
impl DeviceVariant for Variant4 {
    const NUM_LINES: u8 = 4;
}
impl seal::Sealed for Variant4 {}

#[doc(hidden)]
pub struct Variant8;
impl DeviceVariant for Variant8 {
    const NUM_LINES: u8 = 8;
}
impl seal::Sealed for Variant8 {}

pub trait DataModeMarker: seal::Sealed {}

pub struct DataModeUnconfigured;
impl DataModeMarker for DataModeUnconfigured {}
impl seal::Sealed for DataModeUnconfigured {}

pub struct DataMode8Bit;
impl DataModeMarker for DataMode8Bit {}
impl seal::Sealed for DataMode8Bit {}

pub struct DataMode16Bit;
impl DataModeMarker for DataMode16Bit {}
impl seal::Sealed for DataMode16Bit {}

/// Generic driver for all LP586x variants.
pub struct Lp586x<DV, I, DM> {
    interface: I,
    _data_mode: DM,
    _phantom_data: core::marker::PhantomData<DV>,
}

#[cfg(feature = "eh1_0")]
impl<DV: DeviceVariant, DM: DataModeMarker, IE, D> Lp586x<DV, interface::I2cInterface<D>, DM>
where
    D: eh1_0::i2c::I2c<Error = IE>,
{
    pub fn new_with_i2c(
        i2c: D,
        address: u8,
    ) -> Result<Lp586x<DV, interface::I2cInterface<D>, DataModeUnconfigured>, Error<IE>> {
        Lp586x::<DV, _, DataModeUnconfigured>::new(interface::I2cInterface::new(i2c, address))
    }
}

#[cfg(feature = "eh1_0")]
impl<DV: DeviceVariant, DM: DataModeMarker, IE, D> Lp586x<DV, interface::SpiDeviceInterface<D>, DM>
where
    D: eh1_0::spi::SpiDevice<Error = IE>,
{
    pub fn new_with_spi_device(
        spi_device: D,
    ) -> Result<Lp586x<DV, interface::SpiDeviceInterface<D>, DataModeUnconfigured>, Error<IE>> {
        Lp586x::<DV, _, DataModeUnconfigured>::new(interface::SpiDeviceInterface::new(spi_device))
    }
}

#[cfg(not(feature = "eh1_0"))]
impl<DV: DeviceVariant, DM: DataModeMarker, SPI, CS, SPIE>
    Lp586x<DV, interface::SpiInterface<SPI, CS>, DM>
where
    SPI: embedded_hal::blocking::spi::Transfer<u8, Error = SPIE>
        + embedded_hal::blocking::spi::Write<u8, Error = SPIE>,
    CS: embedded_hal::digital::v2::OutputPin,
{
    pub fn new_with_spi_cs(
        spi: SPI,
        cs: CS,
    ) -> Result<
        Lp586x<DV, interface::SpiInterface<SPI, CS>, DataModeUnconfigured>,
        Error<SpiInterfaceError<SPIE, CS::Error>>,
    > {
        Lp586x::<DV, _, DataModeUnconfigured>::new(interface::SpiInterface::new(spi, cs))
    }
}

macro_rules! fault_per_dot_fn {
    ($name:ident, $reg:expr, $doc:literal) => {
        #[doc=$doc]
        pub fn $name(&mut self, dots: &mut [bool]) -> Result<(), Error<IE>> {
            let mut buffer = [0u8; 33];

            self.interface.read_registers($reg, &mut buffer)?;

            dots[..DV::NUM_DOTS as usize]
                .iter_mut()
                .enumerate()
                .map(|(i, dot)| {
                    (
                        i / DV::NUM_CURRENT_SINKS as usize,
                        i % DV::NUM_CURRENT_SINKS as usize,
                        dot,
                    )
                })
                .for_each(|(line, cs, led_is_open)| {
                    *led_is_open = buffer[line * 3 + cs / 8] & (1 << (cs % 8)) > 0;
                });

            Ok(())
        }
    };
}

impl<DV: DeviceVariant, I, DM, IE> Lp586x<DV, I, DM>
where
    I: RegisterAccess<Error = Error<IE>>,
    DM: DataModeMarker,
{
    /// Number of current sinks of the LP586x
    pub const NUM_CURRENT_SINKS: usize = DV::NUM_CURRENT_SINKS as usize;

    /// Total number of LEDs supported by this driver
    pub const NUM_DOTS: usize = DV::NUM_DOTS as usize;

    /// Time to wait after enabling the chip (t_chip_en)
    pub const T_CHIP_EN_US: u32 = 100;

    /// Create a new LP586x driver instance with the given `interface`.
    ///
    /// The returned driver has the chip enabled
    pub fn new(interface: I) -> Result<Lp586x<DV, I, DataModeUnconfigured>, Error<IE>> {
        let mut driver = Lp586x {
            interface,
            _data_mode: DataModeUnconfigured,
            _phantom_data: core::marker::PhantomData,
        };
        driver.reset()?;
        driver.chip_enable(true)?;

        Ok(driver)
    }

    /// Number of lines (switches) supported by this driver
    pub const fn num_lines(&self) -> u8 {
        DV::NUM_LINES
    }

    /// Total number of dots supported by this driver
    pub const fn num_dots(&self) -> u16 {
        DV::NUM_DOTS
    }

    /// Enable or disable the chip.
    ///
    /// After enabling the chip, wait t_chip_en (100µs) for the chip to enter normal mode.
    pub fn chip_enable(&mut self, enable: bool) -> Result<(), Error<IE>> {
        self.interface.write_register(
            Register::CHIP_EN,
            if enable { BitFlags::CHIP_EN_CHIP_EN } else { 0 },
        )
    }

    pub fn configure(&mut self, configuration: &Configuration) -> Result<(), Error<IE>> {
        self.interface.write_registers(
            Register::DEV_INITIAL,
            &[
                configuration.dev_initial_reg_value(),
                configuration.dev_config1_reg_value(),
                configuration.dev_config2_reg_value(),
                configuration.dev_config3_reg_value(),
            ],
        )?;

        Ok(())
    }

    /// Resets the chip.
    pub fn reset(&mut self) -> Result<(), Error<IE>> {
        self.interface.write_register(Register::RESET, 0xff)
    }

    /// Configures dot groups, starting at dot L0-CS0. At least the first dot group has
    /// to be specified, and at most `self.num_dots()`.
    pub fn set_dot_groups(&mut self, dot_groups: &[DotGroup]) -> Result<(), Error<IE>> {
        let mut buffer = [0u8; 54];

        assert!(dot_groups.len() <= self.num_dots() as usize);
        assert!(!dot_groups.is_empty());

        dot_groups
            .iter()
            .enumerate()
            .map(|(i, dot_group)| {
                (
                    i / Self::NUM_CURRENT_SINKS,
                    i % Self::NUM_CURRENT_SINKS,
                    dot_group,
                )
            })
            .for_each(|(line, cs, dot_group)| {
                buffer[line * 5 + cs / 4] |= dot_group.register_value() << (cs % 4 * 2)
            });

        let last_group = (dot_groups.len() - 1) / Self::NUM_CURRENT_SINKS * 5
            + (dot_groups.len() - 1) % Self::NUM_CURRENT_SINKS / 4;

        self.interface
            .write_registers(Register::DOT_GROUP_SELECT_START, &buffer[..=last_group])?;

        Ok(())
    }

    /// Set dot current, starting from `start_dot`.
    pub fn set_dot_current(&mut self, start_dot: u16, current: &[u8]) -> Result<(), Error<IE>> {
        assert!(current.len() <= self.num_dots() as usize);
        assert!(!current.is_empty());

        self.interface
            .write_registers(Register::DOT_CURRENT_START + start_dot, current)?;

        Ok(())
    }

    /// Sets the global brightness across all LEDs.
    pub fn set_global_brightness(&mut self, brightness: u8) -> Result<(), Error<IE>> {
        self.interface
            .write_register(Register::GLOBAL_BRIGHTNESS, brightness)?;

        Ok(())
    }

    /// Sets the brightness across all LEDs in the given [`Group`].
    /// Note that individual LEDS/dots need to be assigned to a `LED_DOT_GROUP`
    /// for this setting to have effect. By default dots ar not assigned to any group.
    pub fn set_group_brightness(&mut self, group: Group, brightness: u8) -> Result<(), Error<IE>> {
        self.interface
            .write_register(group.brightness_reg_addr(), brightness)?;

        Ok(())
    }

    /// Set group current scaling (0..127).
    pub fn set_group_current(&mut self, group: Group, current: u8) -> Result<(), Error<IE>> {
        self.interface
            .write_register(group.current_reg_addr(), current.min(0x7f))?;

        Ok(())
    }

    /// Get global fault state, indicating if any LEDs in the matrix have a
    /// open or short failure.
    pub fn get_global_fault_state(&mut self) -> Result<GlobalFaultState, Error<IE>> {
        let fault_state_value = self.interface.read_register(Register::FAULT_STATE)?;
        Ok(GlobalFaultState::from_reg_value(fault_state_value))
    }

    fault_per_dot_fn!(
        get_led_open_states,
        Register::DOT_LOD_START,
        "Get LED open states, starting from the first dot."
    );

    fault_per_dot_fn!(
        get_led_short_states,
        Register::DOT_LSD_START,
        "Get LED short states, starting from the first dot."
    );

    /// Clear all led open detection (LOD) indication bits
    pub fn clear_led_open_fault(&mut self) -> Result<(), Error<IE>> {
        self.interface.write_register(Register::LOD_CLEAR, 0xF)
    }

    /// Clear all led short detection (LSD) indication bits
    pub fn clear_led_short_fault(&mut self) -> Result<(), Error<IE>> {
        self.interface.write_register(Register::LSD_CLEAR, 0xF)
    }

    pub fn into_16bit_data_mode(self) -> Result<Lp586x<DV, I, DataMode16Bit>, Error<IE>> {
        Ok(Lp586x {
            interface: self.interface,
            _data_mode: DataMode16Bit,
            _phantom_data: core::marker::PhantomData,
        })
    }

    pub fn into_8bit_data_mode(self) -> Result<Lp586x<DV, I, DataMode8Bit>, Error<IE>> {
        Ok(Lp586x {
            interface: self.interface,
            _data_mode: DataMode8Bit,
            _phantom_data: core::marker::PhantomData,
        })
    }
}

/// Trait for accessing PWM data in the correct data format.
pub trait PwmAccess<T> {
    type Error;

    /// Set PWM values of `values.len()` dots, starting from dot `start`.
    fn set_pwm(&mut self, start: u16, values: &[T]) -> Result<(), Self::Error>;

    /// Get PWM value of a single dot.
    fn get_pwm(&mut self, dot: u16) -> Result<T, Self::Error>;
}

impl<DV: DeviceVariant, I, IE> PwmAccess<u8> for Lp586x<DV, I, DataMode8Bit>
where
    I: RegisterAccess<Error = Error<IE>>,
{
    type Error = Error<IE>;

    fn set_pwm(&mut self, start_dot: u16, values: &[u8]) -> Result<(), Self::Error> {
        if values.len() + start_dot as usize > (DV::NUM_DOTS as usize) {
            // TODO: probably we don't want to panic in an embedded system...
            panic!("Too many values supplied for given start and device variant.");
        }

        self.interface
            .write_registers(Register::PWM_BRIGHTNESS_START + start_dot, values)?;

        Ok(())
    }

    fn get_pwm(&mut self, dot: u16) -> Result<u8, Self::Error> {
        self.interface
            .read_register(Register::PWM_BRIGHTNESS_START + dot)
    }
}

impl<DV: DeviceVariant, I, IE> PwmAccess<u16> for Lp586x<DV, I, DataMode16Bit>
where
    I: RegisterAccess<Error = Error<IE>>,
{
    type Error = Error<IE>;

    fn set_pwm(&mut self, start_dot: u16, values: &[u16]) -> Result<(), Self::Error> {
        let mut buffer = [0; Variant0::NUM_DOTS as usize * 2];

        if values.len() + start_dot as usize > (DV::NUM_DOTS as usize) {
            // TODO: probably we don't want to panic in an embedded system...
            panic!("Too many values supplied for given start and device variant.");
        }

        // map u16 values to a u8 buffer (little endian)
        values.iter().enumerate().for_each(|(idx, value)| {
            let register_offset = idx * 2;
            [buffer[register_offset], buffer[register_offset + 1]] = value.to_le_bytes();
        });

        self.interface.write_registers(
            Register::PWM_BRIGHTNESS_START + start_dot * 2,
            &buffer[..values.len() * 2],
        )?;

        Ok(())
    }

    fn get_pwm(&mut self, dot: u16) -> Result<u16, Self::Error> {
        self.interface
            .read_register_wide(Register::PWM_BRIGHTNESS_START + (dot * 2))
    }
}

#[cfg(feature = "eh1_0")]
impl<DV, SPID: eh1_0::spi::SpiDevice, DM> Lp586x<DV, interface::SpiDeviceInterface<SPID>, DM> {
    /// Destroys the driver and releases the owned [`SpiDevice`].
    pub fn release(self) -> SPID {
        self.interface.release()
    }
}

#[cfg(feature = "eh1_0")]
impl<DV, I2C: eh1_0::i2c::I2c, DM> Lp586x<DV, interface::I2cInterface<I2C>, DM> {
    /// Destorys the driver and releases the owned [`I2c`].
    pub fn release(self) -> I2C {
        self.interface.release()
    }
}

#[cfg(test)]
impl<DV, DM> Lp586x<DV, interface::mock::MockInterface, DM> {
    /// Destroys the drivers and returns the owned [`MockInterface`].
    pub fn release(self) -> interface::mock::MockInterface {
        self.interface
    }
}

/// LP5860 driver with 11 lines
pub type Lp5860<I> = Lp586x<Variant0, I, DataModeUnconfigured>;

/// LP5861 driver with 1 line
pub type Lp5861<I> = Lp586x<Variant1, I, DataModeUnconfigured>;

/// LP5862 driver with 2 lines
pub type Lp5862<I> = Lp586x<Variant2, I, DataModeUnconfigured>;

/// LP5864 driver with 4 lines
pub type Lp5864<I> = Lp586x<Variant4, I, DataModeUnconfigured>;

/// LP5868 driver with 8 lines
pub type Lp5868<I> = Lp586x<Variant8, I, DataModeUnconfigured>;

#[cfg(test)]
mod tests {
    use super::*;
    use interface::mock::{Access, MockInterface};

    #[test]
    fn test_create_new() {
        let interface = MockInterface::new(vec![
            Access::WriteRegister(0x0a9, 0xff),
            Access::WriteRegister(0x000, 1),
        ]);

        let ledmatrix = Lp5860::new(interface).unwrap();

        ledmatrix.release().done();
    }

    #[test]
    fn test_set_dot_groups() {
        #[rustfmt::skip]
        let interface = MockInterface::new(vec![
            Access::WriteRegister(0x0a9, 0xff),
            Access::WriteRegister(0x000, 1),
            Access::WriteRegisters(
                0x00c,
                vec![
                    // L0
                    0b01111001, 0b10011110, 0b11100111, 0b01111001, 0b1110,
                    // L1
                    0b01111001, 0b00111110,
                ],
            ),
            Access::WriteRegisters(
                0x00c,
                vec![0b00]
            ),
        ]);

        let mut ledmatrix = Lp5860::new(interface).unwrap();

        ledmatrix
            .set_dot_groups(&[
                // L0
                DotGroup::Group0,
                DotGroup::Group1,
                DotGroup::Group2,
                DotGroup::Group0,
                DotGroup::Group1,
                DotGroup::Group2,
                DotGroup::Group0,
                DotGroup::Group1,
                DotGroup::Group2,
                DotGroup::Group0,
                DotGroup::Group1,
                DotGroup::Group2,
                DotGroup::Group0,
                DotGroup::Group1,
                DotGroup::Group2,
                DotGroup::Group0,
                DotGroup::Group1,
                DotGroup::Group2,
                // L1
                DotGroup::Group0,
                DotGroup::Group1,
                DotGroup::Group2,
                DotGroup::Group0,
                DotGroup::Group1,
                DotGroup::Group2,
                DotGroup::Group2,
            ])
            .unwrap();

        ledmatrix.set_dot_groups(&[DotGroup::None]).unwrap();

        ledmatrix.release().done();
    }
}
