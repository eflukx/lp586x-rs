use crate::Error;

/// Trait for giving read and write access to registers
pub trait RegisterAccess {
    type Error;

    /// Reads `N` values from multiple registers, starting from `start_register` and incrementing
    /// the register for every elements.
    fn read_registers(&mut self, start_register: u16, data: &mut [u8]) -> Result<(), Self::Error>;

    /// Reads a single value from `register`.
    fn read_register(&mut self, register: u16) -> Result<u8, Self::Error> {
        let mut buffer: [u8; 1] = [0; 1];
        self.read_registers(register, &mut buffer)?;

        Ok(buffer[0])
    }

    /// Writes to multiple registers, starting from `start_register` and incrementing
    /// the register by one for every element in `data`.
    fn write_registers(&mut self, start_register: u16, data: &[u8]) -> Result<(), Self::Error>;

    /// Writes a single value to `register`.
    fn write_register(&mut self, register: u16, data: u8) -> Result<(), Self::Error> {
        self.write_registers(register, &[data])
    }
}

const fn spi_transmission_header(register: u16, write: bool) -> [u8; 2] {
    [
        (register >> 2) as u8,
        (register << 6) as u8 | if write { 1 << 5 } else { 0 },
    ]
}

use embedded_hal::{blocking::spi, digital::v2::OutputPin};

pub struct SpiInterface<SPI, CS> {
    pub(crate) spi: SPI,
    pub(crate) cs: CS,
}

impl<SPI: spi::Transfer<u8> + spi::Write<u8>, CS: OutputPin> SpiInterface<SPI, CS> {
    pub fn new(spi: SPI, cs: CS) -> Self {
        Self { spi, cs }
    }

    pub fn release(self) -> (SPI, CS) {
        (self.spi, self.cs)
    }
}

impl<SPI, CS, BusE> RegisterAccess for SpiInterface<SPI, CS>
where
    SPI: spi::Transfer<u8, Error = BusE> + spi::Write<u8, Error = BusE>,
    CS: OutputPin<Error = BusE>,
{
    type Error = Error<BusE>;

    fn read_registers(&mut self, start_register: u16, data: &mut [u8]) -> Result<(), Self::Error> {
        let header = spi_transmission_header(start_register, false);

        self.cs.set_low().map_err(Error::Bus)?;

        self.spi.write(&header).map_err(Error::Bus)?;

        self.spi.transfer(data).map_err(Error::Bus)?;

        self.cs.set_high().map_err(Error::Bus)?;

        Ok(())
    }

    fn write_registers(&mut self, start_register: u16, data: &[u8]) -> Result<(), Self::Error> {
        let header = spi_transmission_header(start_register, true);

        self.cs.set_low().map_err(Error::Bus)?;

        self.spi.write(&header).map_err(Error::Bus)?;
        self.spi.write(data).map_err(Error::Bus)?;

        self.cs.set_high().map_err(Error::Bus)?;

        Ok(())
    }
}

#[cfg(feature = "eh1_0")]
mod for_eh1_0 {
    use super::*;
    use eh1_0::{i2c, spi};

    pub struct SpiDeviceInterface<SPID> {
        pub(crate) spi_device: SPID,
    }

    impl<SPID: spi::SpiDevice> SpiDeviceInterface<SPID> {
        pub fn new(spi_device: SPID) -> Self {
            Self { spi_device }
        }

        pub fn release(self) -> SPID {
            self.spi_device
        }
    }

    impl<SPID, BusE> RegisterAccess for SpiDeviceInterface<SPID>
    where
        SPID: spi::SpiDevice<Error = BusE>,
    {
        type Error = Error<BusE>;

        fn read_registers(
            &mut self,
            start_register: u16,
            data: &mut [u8],
        ) -> Result<(), Self::Error> {
            let header = spi_transmission_header(start_register, false);

            let mut operations = [spi::Operation::Write(&header), spi::Operation::Read(data)];

            self.spi_device
                .transaction(&mut operations)
                .map_err(Error::Bus)?;

            Ok(())
        }

        fn write_registers(&mut self, start_register: u16, data: &[u8]) -> Result<(), Self::Error> {
            let header = spi_transmission_header(start_register, true);

            let mut operations = [spi::Operation::Write(&header), spi::Operation::Write(data)];

            self.spi_device
                .transaction(&mut operations)
                .map_err(Error::Bus)?;

            Ok(())
        }
    }

    pub struct I2cInterface<I2C> {
        pub(crate) i2c: I2C,
        pub(crate) address: u8,
    }

    #[cfg(feature = "eh1_0")]
    impl<I2C: i2c::I2c> I2cInterface<I2C> {
        pub fn new(i2c: I2C, address: u8) -> Self {
            Self { i2c, address }
        }

        pub fn release(self) -> I2C {
            self.i2c
        }

        fn address_with_register(&self, register: u16) -> u8 {
            (self.address & !0b111) | ((register & 0x300) >> 7) as u8
        }
    }

    #[cfg(feature = "eh1_0")]
    impl<I2C, BusE> RegisterAccess for I2cInterface<I2C>
    where
        I2C: i2c::I2c<Error = BusE>,
    {
        type Error = Error<BusE>;

        fn read_registers(
            &mut self,
            start_register: u16,
            data: &mut [u8],
        ) -> Result<(), Self::Error> {
            let header = [(start_register & 0xff) as u8];
            let mut operations = [i2c::Operation::Write(&header), i2c::Operation::Read(data)];

            self.i2c
                .transaction(self.address_with_register(start_register), &mut operations)
                .map_err(Error::Bus)?;

            Ok(())
        }

        fn write_registers(&mut self, start_register: u16, data: &[u8]) -> Result<(), Self::Error> {
            let header = [(start_register & 0xff) as u8];

            let mut operations = [i2c::Operation::Write(&header), i2c::Operation::Write(data)];

            self.i2c
                .transaction(self.address_with_register(start_register), &mut operations)
                .map_err(Error::Bus)?;

            Ok(())
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use embedded_hal_mock::{
            i2c::{Mock as I2cMock, Transaction as I2cTransaction},
            spi::{Mock as SpiMock, Transaction as SpiTransaction},
        };

        #[test]
        fn test_spi_read_register() {
            // test writing to register 0x38b
            const REGISTER: u16 = 0x38b;
            const VALUE: u8 = 0xAB;

            let spi = SpiMock::new(&[
                SpiTransaction::transaction_start(),
                SpiTransaction::write_vec(vec![0xe2, 0xc0]),
                SpiTransaction::read(VALUE),
                SpiTransaction::transaction_end(),
            ]);

            let mut spi_if = SpiDeviceInterface::new(spi);

            let value = spi_if.read_register(REGISTER).unwrap();

            assert_eq!(value, VALUE);

            spi_if.release().done();
        }

        #[test]
        fn test_spi_write_register() {
            // test writing to register 0x38b
            const REGISTER: u16 = 0x38b;
            const VALUE: u8 = 0xAB;

            let spi = SpiMock::new(&[
                SpiTransaction::transaction_start(),
                SpiTransaction::write_vec(vec![0xe2, 0xe0]),
                SpiTransaction::write(VALUE),
                SpiTransaction::transaction_end(),
            ]);

            let mut spi_if = SpiDeviceInterface::new(spi);

            spi_if.write_register(REGISTER, VALUE).unwrap();

            spi_if.release().done();
        }

        #[test]
        fn test_i2c_read_register() {
            // test writing to register 0x38b
            const REGISTER: u16 = 0x38b;
            const VALUE: u8 = 0xAB;

            let i2c = I2cMock::new(&[
                I2cTransaction::transaction_start(0),
                I2cTransaction::write(0, vec![0x8b]),
                I2cTransaction::read(0, vec![VALUE]),
                I2cTransaction::transaction_end(0),
            ]);

            let mut i2c_if = I2cInterface::new(i2c, 0);

            let value = i2c_if.read_register(REGISTER).unwrap();
            assert_eq!(value, VALUE);

            i2c_if.release().done();
        }

        #[test]
        fn test_i2c_write_register() {
            // test writing to register 0x38b
            const REGISTER: u16 = 0x38b;
            const VALUE: u8 = 0xAB;

            let i2c = I2cMock::new(&[
                I2cTransaction::transaction_start(0),
                I2cTransaction::write(0, vec![0x8b, VALUE]),
                I2cTransaction::transaction_end(0),
            ]);

            let mut i2c_if = I2cInterface::new(i2c, 0);

            i2c_if.write_register(REGISTER, VALUE).unwrap();

            i2c_if.release().done();
        }
    }
}

#[cfg(feature = "eh1_0")]
pub use for_eh1_0::{I2cInterface, SpiDeviceInterface};

#[cfg(test)]
pub(crate) mod mock {
    use super::RegisterAccess;
    use crate::Error;

    #[derive(Debug)]
    #[allow(dead_code)]
    pub(crate) enum Access {
        ReadRegister(u16, u8),
        ReadRegisters(u16, Vec<u8>),
        WriteRegister(u16, u8),
        WriteRegisters(u16, Vec<u8>),
    }

    #[derive(Debug)]
    pub(crate) struct MockInterface {
        expected_accesses: Vec<Access>,
    }

    impl MockInterface {
        pub fn new(mut accesses: Vec<Access>) -> Self {
            // reverse order so we can just pop() them
            accesses.reverse();

            Self {
                expected_accesses: accesses,
            }
        }

        pub fn done(&self) {
            assert!(
                self.expected_accesses.is_empty(),
                "Not all expected register accesses were executed"
            );
        }
    }

    impl RegisterAccess for MockInterface {
        type Error = Error<()>;

        fn read_registers(
            &mut self,
            start_register: u16,
            data: &mut [u8],
        ) -> Result<(), Self::Error> {
            match self.expected_accesses.pop() {
                Some(Access::ReadRegister(reg, read_data)) if data.len() == 1 => {
                    assert_eq!(
                        reg, start_register,
                        "Expected read on register {reg:x} but got {start_register:x}."
                    );

                    data[0] = read_data;
                }
                Some(Access::ReadRegisters(reg, read_data)) => {
                    assert_eq!(
                        reg, start_register,
                        "Expected reads on register {reg:x} but got {start_register:x}."
                    );
                    data.copy_from_slice(&read_data[..]);
                }
                Some(access) => {
                    panic!("Unexpected register access when expecting ReadRegisters: {access:?}")
                }
                None => panic!("Register access beyond the list of expected register accesses"),
            };

            Ok(())
        }

        fn write_registers(&mut self, start_register: u16, data: &[u8]) -> Result<(), Self::Error> {
            match self.expected_accesses.pop() {
                Some(Access::WriteRegister(reg, expected_value)) if data.len() == 1 => {
                    let data = data[0];

                    assert_eq!(
                        reg, start_register,
                        "Expected write on register {reg:x} but got {start_register:x}"
                    );
                    assert_eq!(expected_value, data, "Expected data written to register {reg:x} to be {expected_value:x} but got {data:x}");
                }
                Some(Access::WriteRegisters(reg, expected_values)) => {
                    assert_eq!(
                        reg, start_register,
                        "Expected writes on register {reg:x} but got {start_register:x}"
                    );
                    assert_eq!(expected_values, data, "Expected data written to register {reg:x} to be {expected_values:x?} but got {data:x?}");
                }
                Some(access) => {
                    panic!("Unexpected register access when expecting WriteRegisters: {access:?}")
                }
                _ => panic!("Register access beyond the list of expected register accesses"),
            };

            Ok(())
        }
    }
}
