mod headers;
mod request;

use crate::{
    SubDevice, SubDeviceRef,
    error::{
        Error::{self, SoeError},
        PduError,
    },
    fmt,
    mailbox::{
        MailboxType,
        soe::{
            headers::{SoeElementFlag, SoeErrorFlag},
            request::IdnHeader,
        },
    },
    pdu_loop::ReceivedPdu,
};
use core::ops::Deref;
use core::{any::type_name, fmt::Debug};
use ethercrab_wire::{EtherCrabWireRead, EtherCrabWireSized, EtherCrabWireWrite};
use std::println;

pub(crate) use headers::{SoeHeader, SoeOpcode};

pub struct Soe<'maindevice, S> {
    subdevice: &'maindevice SubDeviceRef<'maindevice, S>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum SoeErrorCode {
    Success = 0x0000,
    IdnDoesNotExist = 0x0001,
    InvalidIdnFormat = 0x0009,
    ServiceNotSupported = 0x0010,
    DataContainerTooSmall = 0x0011,
    ElementDoesNotExist = 0x0020,
    NameElementDoesNotExist = 0x0023,
    ElementNotSupported = 0x0028,
    WriteAccessProhibited = 0x1001,
    WriteBlockedByPhase = 0x1002,
    ValueOutsideLimits = 0x1009,
    InvalidValue = 0x100A,
    CommandExecutionError = 0x2001,
    Unknown(u16),
}

impl From<u16> for SoeErrorCode {
    fn from(code: u16) -> Self {
        match code {
            0x0000 => Self::Success,
            0x0001 => Self::IdnDoesNotExist,
            0x0009 => Self::InvalidIdnFormat,
            0x0010 => Self::ServiceNotSupported,
            0x0011 => Self::DataContainerTooSmall,
            0x0020 => Self::ElementDoesNotExist,
            0x0023 => Self::NameElementDoesNotExist,
            0x0028 => Self::ElementNotSupported,
            0x1001 => Self::WriteAccessProhibited,
            0x1002 => Self::WriteBlockedByPhase,
            0x1009 => Self::ValueOutsideLimits,
            0x100A => Self::InvalidValue,
            0x2001 => Self::CommandExecutionError,
            other => Self::Unknown(other),
        }
    }
}

impl<'maindevice, S> Soe<'maindevice, S>
where
    S: Deref<Target = SubDevice>,
{
    pub fn new(subdevice: &'maindevice SubDeviceRef<'maindevice, S>) -> Self {
        Self { subdevice }
    }

    /// Send a mailbox request, wait for response mailbox to be ready, read response from mailbox
    /// and return as a slice.
    async fn mailbox_write_read<R>(
        &'maindevice self,
        header: IdnHeader,
        data: R,
    ) -> Result<(IdnHeader, ReceivedPdu<'maindevice>), Error>
    where
        R: EtherCrabWireWrite + Debug,
    {
        let (read_mailbox, write_mailbox) =
            self.subdevice
                .wait_for_mailboxes()
                .await
                .inspect_err(|err| {
                    fmt::error!(
                        "{} {} {}",
                        self.subdevice.configured_address(),
                        self.subdevice.name(),
                        err
                    )
                })?;

        let header_len = header.packed_len();
        let data_len = data.packed_len();
        let total_len = header_len + data_len;

        let mut request = vec![0u8; total_len];
        header.pack_to_slice(&mut request[..header_len])?;

        let data: &[u8] = &request;
        println!("Request: ");
        println!("{:?}", &data);
        // header.pack_to_slice(&mut request[header_len..])?;

        // Send data to SubDevice IN mailbox
        self.subdevice
            .write(write_mailbox.address)
            .with_len(write_mailbox.len)
            .send(self.subdevice.maindevice, &request[..])
            .await?;

        let mut response = self
            .subdevice
            .wait_for_mailbox_response(&read_mailbox)
            .await?;

        let data: &[u8] = &response;
        println!("Response: ");
        println!("{:?}", &data);
        let headers = IdnHeader::unpack_from_slice(&response)?;
        println!("response length: {}", headers.mailbox_header.length);
        response.trim_front(IdnHeader::PACKED_LEN);

        // Check the response header error bit
        match headers.soe_header.error {
            SoeErrorFlag::NoError => {}
            SoeErrorFlag::ErrorOccurred => {
                let payload: &[u8] = &response;
                let error_code = u16::from_le_bytes([payload[0], payload[1]]);
                println!("{}", error_code);

                return Err(SoeError(SoeErrorCode::from(error_code)));
            }
        }

        // TODO!
        // Validate that the mailbox response is to the request we just sent
        if headers.mailbox_header.mailbox_type != MailboxType::Soe
        // || !request.validate_response(headers.address)
        {
            // fmt::error!(
            //     "Invalid SDO response. Type: {:?} (expected {:?}), index {}, subindex {}",
            //     headers.header.mailbox_type,
            //     MailboxType::Soe,
            //     headers.address,
            // );

            // Err(Error::Mailbox(MailboxError::SdoResponseInvalid {
            //     address: headers.address,
            // }))
        }
        // let headers = IdnHeader::unpack_from_slice(&response)?;

        Ok((headers, response))
    }

    pub async fn idn_read_flag<T>(
        &self,
        drive_num: u8,
        idn_address: u16,
        flag: SoeElementFlag,
    ) -> Result<T, Error>
    where
        T: EtherCrabWireRead,
    {
        let counter = self.subdevice.mailbox_counter();

        let header = IdnHeader::for_reading(counter, drive_num, idn_address, flag);

        let (headers, response) = self.mailbox_write_read(header, []).await?;

        let l = (headers.mailbox_header.length as usize) - SoeHeader::PACKED_LEN;

        let data: &[u8] = &response[..l];

        T::unpack_from_slice(data).map_err(|_| {
            fmt::error!(
                "SDO expedited data decode T: {}, data {:?} (len {})",
                type_name::<T>(),
                data,
                data.len(),
            );

            Error::Pdu(PduError::Decode)
        })
    }

    pub async fn idn_read_status(&self, drive_num: u8, idn_address: u16) -> Result<u16, Error> {
        self.idn_read_flag::<u16>(drive_num, idn_address, SoeElementFlag::DataStateStatus)
            .await
    }

    pub async fn idn_read_name(&self, drive_num: u8, idn_address: u16) -> Result<String, Error> {
        let counter = self.subdevice.mailbox_counter();

        let header = IdnHeader::for_reading(
            counter,
            drive_num,
            idn_address,
            SoeElementFlag::NameDescriptor,
        );

        let (headers, response) = self.mailbox_write_read(header, []).await?;

        let l = (headers.mailbox_header.length as usize) - SoeHeader::PACKED_LEN;

        let data: &[u8] = &response[..l];

        Ok(str::from_utf8(data)
            .map_err(|e| {
                fmt::error!(
                    "SDO expedited data decode T: {} (len {}) data {:?} (len {})",
                    type_name::<String>(),
                    data.len(),
                    data,
                    data.len(),
                );
                println!("{:?}", e);

                Error::Pdu(PduError::Decode)
            })?
            .to_owned())
    }

    pub async fn idn_read_attribute(&self, drive_num: u8, idn_address: u16) -> Result<u32, Error> {
        self.idn_read_flag::<u32>(drive_num, idn_address, SoeElementFlag::Attribute)
            .await
    }

    pub async fn idn_read_units(&self, drive_num: u8, idn_address: u16) -> Result<String, Error> {
        let counter = self.subdevice.mailbox_counter();

        let header = IdnHeader::for_reading(counter, drive_num, idn_address, SoeElementFlag::Unit);

        let (headers, response) = self.mailbox_write_read(header, []).await?;

        let l = (headers.mailbox_header.length as usize) - SoeHeader::PACKED_LEN;

        let data: &[u8] = &response[..l];

        String::unpack_from_slice(data).map_err(|_| {
            fmt::error!(
                "SDO expedited data decode T: {} (len {}) data {:?} (len {})",
                type_name::<String>(),
                data.len(),
                data,
                data.len(),
            );

            Error::Pdu(PduError::Decode)
        })
    }

    pub async fn idn_read_min<T>(&self, drive_num: u8, idn_address: u16) -> Result<T, Error>
    where
        T: EtherCrabWireRead,
    {
        self.idn_read_flag::<T>(drive_num, idn_address, SoeElementFlag::MinimumValue)
            .await
    }
    pub async fn idn_read_max<T>(&self, drive_num: u8, idn_address: u16) -> Result<T, Error>
    where
        T: EtherCrabWireRead,
    {
        self.idn_read_flag::<T>(drive_num, idn_address, SoeElementFlag::MaximumValue)
            .await
    }
    pub async fn idn_read_data<T>(&self, drive_num: u8, idn_address: u16) -> Result<T, Error>
    where
        T: EtherCrabWireRead,
    {
        self.idn_read_flag::<T>(drive_num, idn_address, SoeElementFlag::ValueData)
            .await
    }
    pub async fn idn_read_default<T>(&self, drive_num: u8, idn_address: u16) -> Result<T, Error>
    where
        T: EtherCrabWireRead,
    {
        self.idn_read_flag::<T>(drive_num, idn_address, SoeElementFlag::DefaultValue)
            .await
    }

    pub async fn idn_write_data<T>(
        &self,
        drive_num: u8,
        idn_address: u16,
        value: T,
    ) -> Result<(), Error>
    where
        T: EtherCrabWireWrite + Debug,
    {
        let counter = self.subdevice.mailbox_counter();

        let header = IdnHeader::for_writing(
            counter,
            drive_num,
            idn_address,
            SoeElementFlag::Unit,
            value.packed_len() as u16,
        );

        let (_, _) = self.mailbox_write_read(header, value).await?;

        Ok(())
    }
}

//TODO
#[allow(missing_docs)]
#[macro_export]
macro_rules! idn {
    (S, $group:expr, $number:expr) => {{
        const G: u16 = ($group & 0x07) << 12;
        const N: u16 = $number & 0x0FFF;
        G | N
    }};
    (P, $group:expr, $number:expr) => {{
        const SET: u16 = 1 << 15;
        const G: u16 = ($group & 0x07) << 12;
        const N: u16 = $number & 0x0FFF;
        SET | G | N
    }};
}
