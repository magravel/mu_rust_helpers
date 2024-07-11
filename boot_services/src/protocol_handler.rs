use core::{ffi::c_void, mem, ptr, slice};

use r_efi::efi::{self, Event, Guid};

use crate::StandardBootServices;

pub trait ProtocolInterface {
  fn protocol_guid(&self) -> &efi::Guid;
}

pub trait ProtocolHandlerServices {
  unsafe fn install_protocol_interface(
    &self,
    handle: Option<efi::Handle>,
    protocol: &'static efi::Guid,
    interface: Option<*mut c_void>,
  ) -> Result<efi::Handle, efi::Status>;

  unsafe fn uninstall_protocol_interface(
    &self,
    handle: efi::Handle,
    protocol: &'static efi::Guid,
    interface: Option<*mut c_void>,
  ) -> Result<(), efi::Status>;

  unsafe fn reinstall_protocol_interface(
    &self,
    handle: efi::Handle,
    protocol: &'static efi::Guid,
    old_protocol_interface: Option<*mut c_void>,
    new_protocol_interface: Option<*mut c_void>,
  ) -> Result<(), efi::Status>;

  unsafe fn register_protocol_notify(
    &self,
    protocol: &efi::Guid,
    event: Event,
    registration: *mut *mut c_void,
  ) -> Result<(), efi::Status>;

  fn locate_handle<'a>(
    &self,
    search_type: HandleSearchType<'_>,
    buffer: &'a mut [u8],
  ) -> Result<&'a [efi::Handle], efi::Status>;

  fn handle_protocol(&self, handle: efi::Handle, protocol: &efi::Guid) -> Result<*mut c_void, efi::Status>;

  fn locate_device_path(
    &self,
    protocol: &efi::Guid,
    device_path: *mut *mut efi::protocols::device_path::Protocol,
  ) -> Result<efi::Handle, efi::Status>;

  fn open_protocol(
    &self,
    handle: efi::Handle,
    protocol: &efi::Guid,
    agent_handle: efi::Handle,
    controller_handle: efi::Handle,
    attribute: u32,
  ) -> Result<*mut c_void, efi::Status>;

  fn close_protocol(
    &self,
    handle: efi::Handle,
    protocol: &efi::Guid,
    agent_handle: efi::Handle,
    controller_handle: efi::Handle,
  ) -> Result<(), efi::Status>;

  fn open_protocol_information(
    &self,
    handle: efi::Handle,
    protocol: &efi::Guid,
  ) -> Result<&'static [efi::OpenProtocolInformationEntry], efi::Status>;

  unsafe fn connect_controller(
    &self,
    controller_handle: efi::Handle,
    driver_image_handle: Option<*mut efi::Handle>,
    remaining_device_path: Option<&mut efi::protocols::device_path::Protocol>,
    recursive: bool,
  ) -> Result<(), efi::Status>;

  fn disconnect_controller(
    &self,
    controller_handle: efi::Handle,
    driver_image_handle: Option<efi::Handle>,
    child_handle: Option<efi::Handle>,
  ) -> Result<(), efi::Status>;

  fn protocols_per_handle(&self, handle: efi::Handle) -> Result<&'static [efi::Guid], efi::Status>;

  fn locate_handle_buffer(&self, search_type: HandleSearchType<'_>) -> Result<&'static [efi::Handle], efi::Status>;

  fn locate_protocol(
    &self,
    protocol: &'static efi::Guid,
    registration: Option<*mut c_void>,
  ) -> Result<*mut c_void, efi::Status>;
}

impl ProtocolHandlerServices for StandardBootServices<'_> {
  unsafe fn install_protocol_interface(
    &self,
    handle: Option<efi::Handle>,
    protocol: &'static efi::Guid,
    interface: Option<*mut c_void>,
  ) -> Result<efi::Handle, efi::Status> {
    let mut handle = handle.unwrap_or(ptr::null_mut());
    match (self.efi_boot_services().install_protocol_interface)(
      ptr::addr_of_mut!(handle),
      protocol as *const _ as *mut _,
      efi::NATIVE_INTERFACE,
      interface.unwrap_or(ptr::null_mut()),
    ) {
      s if s.is_error() => Err(s),
      _ => Ok(handle),
    }
  }

  unsafe fn uninstall_protocol_interface(
    &self,
    handle: efi::Handle,
    protocol: &'static efi::Guid,
    interface: Option<*mut c_void>,
  ) -> Result<(), efi::Status> {
    match (self.efi_boot_services().uninstall_protocol_interface)(
      handle,
      protocol as *const _ as *mut _,
      interface.unwrap_or(ptr::null_mut()),
    ) {
      s if s.is_error() => Err(s),
      _ => Ok(()),
    }
  }

  unsafe fn reinstall_protocol_interface(
    &self,
    handle: efi::Handle,
    protocol: &'static efi::Guid,
    old_protocol_interface: Option<*mut c_void>,
    new_protocol_interface: Option<*mut c_void>,
  ) -> Result<(), efi::Status> {
    match (self.efi_boot_services().reinstall_protocol_interface)(
      handle,
      protocol as *const _ as *mut _,
      old_protocol_interface.unwrap_or(ptr::null_mut()),
      new_protocol_interface.unwrap_or(ptr::null_mut()),
    ) {
      s if s.is_error() => Err(s),
      _ => Ok(()),
    }
  }

  unsafe fn register_protocol_notify(
    &self,
    protocol: &efi::Guid,
    event: Event,
    registration: *mut *mut c_void,
  ) -> Result<(), efi::Status> {
    match (self.efi_boot_services().register_protocol_notify)(protocol as *const _ as *mut _, event, registration) {
      s if s.is_error() => Err(s),
      _ => Ok(()),
    }
  }

  fn locate_handle<'a>(
    &self,
    search_type: HandleSearchType<'_>,
    buffer: &'a mut [u8],
  ) -> Result<&'a [efi::Handle], efi::Status> {
    let protocol = match search_type {
      HandleSearchType::ByProtocol(p) => p as *const _ as *mut _,
      _ => ptr::null_mut(),
    };
    let search_key = match search_type {
      HandleSearchType::ByRegisterNotify(k) => k,
      _ => ptr::null_mut(),
    };
    let mut buffer_size = buffer.len();
    match (self.efi_boot_services().locate_handle)(
      search_type.into(),
      protocol,
      search_key,
      ptr::addr_of_mut!(buffer_size),
      buffer.as_mut_ptr() as *mut efi::Handle,
    ) {
      s if s.is_error() => Err(s),
      _ => {
        Ok(unsafe { slice::from_raw_parts(buffer.as_ptr() as *const _, buffer_size / mem::size_of::<efi::Handle>()) })
      }
    }
  }

  fn handle_protocol(&self, handle: efi::Handle, protocol: &efi::Guid) -> Result<*mut c_void, efi::Status> {
    let mut interface = ptr::null_mut();
    match (self.efi_boot_services().handle_protocol)(
      handle,
      protocol as *const _ as *mut _,
      ptr::addr_of_mut!(interface),
    ) {
      s if s.is_error() => Err(s),
      _ => Ok(interface),
    }
  }

  fn locate_device_path(
    &self,
    protocol: &efi::Guid,
    device_path: *mut *mut efi::protocols::device_path::Protocol,
  ) -> Result<efi::Handle, efi::Status> {
    let mut device = ptr::null_mut();
    match (self.efi_boot_services().locate_device_path)(
      protocol as *const _ as *mut _,
      device_path,
      ptr::addr_of_mut!(device),
    ) {
      s if s.is_error() => Err(s),
      _ => Ok(device),
    }
  }

  fn open_protocol(
    &self,
    handle: efi::Handle,
    protocol: &efi::Guid,
    agent_handle: efi::Handle,
    controller_handle: efi::Handle,
    attribute: u32,
  ) -> Result<*mut c_void, efi::Status> {
    let mut interface = ptr::null_mut();
    match (self.efi_boot_services().open_protocol)(
      handle,
      protocol as *const _ as *mut _,
      ptr::addr_of_mut!(interface),
      agent_handle,
      controller_handle,
      attribute,
    ) {
      s if s.is_error() => Err(s),
      _ => Ok(interface),
    }
  }

  fn close_protocol(
    &self,
    handle: efi::Handle,
    protocol: &efi::Guid,
    agent_handle: efi::Handle,
    controller_handle: efi::Handle,
  ) -> Result<(), efi::Status> {
    match (self.efi_boot_services().close_protocol)(
      handle,
      protocol as *const _ as *mut _,
      agent_handle,
      controller_handle,
    ) {
      s if s.is_error() => Err(s),
      _ => Ok(()),
    }
  }

  fn open_protocol_information(
    &self,
    handle: efi::Handle,
    protocol: &efi::Guid,
  ) -> Result<&'static [efi::OpenProtocolInformationEntry], efi::Status> {
    let mut entry_buffer = ptr::null_mut();
    let mut entry_count = 0;
    match (self.efi_boot_services().open_protocol_information)(
      handle,
      protocol as *const _ as *mut _,
      ptr::addr_of_mut!(entry_buffer),
      ptr::addr_of_mut!(entry_count),
    ) {
      s if s.is_error() => Err(s),
      _ => Ok(unsafe { slice::from_raw_parts(entry_buffer, entry_count) }),
    }
  }

  unsafe fn connect_controller(
    &self,
    controller_handle: efi::Handle,
    driver_image_handle: Option<*mut efi::Handle>,
    remaining_device_path: Option<&mut efi::protocols::device_path::Protocol>,
    recursive: bool,
  ) -> Result<(), efi::Status> {
    match (self.efi_boot_services().connect_controller)(
      controller_handle,
      driver_image_handle.unwrap_or(ptr::null_mut()),
      remaining_device_path.map_or(ptr::null_mut(), |x| x as *mut _),
      recursive.into(),
    ) {
      s if s.is_error() => Err(s),
      _ => Ok(()),
    }
  }

  fn disconnect_controller(
    &self,
    controller_handle: efi::Handle,
    driver_image_handle: Option<efi::Handle>,
    child_handle: Option<efi::Handle>,
  ) -> Result<(), efi::Status> {
    match (self.efi_boot_services().disconnect_controller)(
      controller_handle,
      driver_image_handle.unwrap_or(ptr::null_mut()),
      child_handle.unwrap_or(ptr::null_mut()),
    ) {
      s if s.is_error() => Err(s),
      _ => Ok(()),
    }
  }

  fn protocols_per_handle(&self, handle: efi::Handle) -> Result<&'static [efi::Guid], efi::Status> {
    let mut protocol_buffer = ptr::null_mut();
    let mut protocol_buffer_count = 0;
    match (self.efi_boot_services().protocols_per_handle)(
      handle,
      ptr::addr_of_mut!(protocol_buffer),
      ptr::addr_of_mut!(protocol_buffer_count),
    ) {
      s if s.is_error() => Err(s),
      _ => Ok(unsafe { slice::from_raw_parts(protocol_buffer as *const _, protocol_buffer_count) }),
    }
  }

  fn locate_handle_buffer(&self, search_type: HandleSearchType<'_>) -> Result<&'static [efi::Handle], efi::Status> {
    let mut buffer = ptr::null_mut();
    let mut buffer_count = 0;
    let protocol = match search_type {
      HandleSearchType::ByProtocol(p) => p as *const _ as *mut _,
      _ => ptr::null_mut(),
    };
    let search_key = match search_type {
      HandleSearchType::ByRegisterNotify(k) => k,
      _ => ptr::null_mut(),
    };
    match (self.efi_boot_services().locate_handle_buffer)(
      search_type.into(),
      protocol,
      search_key,
      ptr::addr_of_mut!(buffer_count),
      ptr::addr_of_mut!(buffer),
    ) {
      s if s.is_error() => Err(s),
      _ => Ok(unsafe { slice::from_raw_parts(buffer as *const _, buffer_count) }),
    }
  }

  fn locate_protocol(
    &self,
    protocol: &'static efi::Guid,
    registration: Option<*mut c_void>,
  ) -> Result<*mut c_void, efi::Status> {
    let mut interface = ptr::null_mut();
    match (self.efi_boot_services().locate_protocol)(
      protocol as *const _ as *mut _,
      registration.unwrap_or(ptr::null_mut()),
      ptr::addr_of_mut!(interface),
    ) {
      s if s.is_error() => Err(s),
      _ => Ok(interface),
    }
  }
}

pub enum HandleSearchType<'a> {
  AllHandle,
  ByRegisterNotify(*mut c_void), // todo find a way to put a better type here
  ByProtocol(&'a Guid),
}

impl Into<efi::LocateSearchType> for HandleSearchType<'_> {
  fn into(self) -> efi::LocateSearchType {
    match self {
      HandleSearchType::AllHandle => efi::ALL_HANDLES,
      HandleSearchType::ByRegisterNotify(_) => efi::BY_REGISTER_NOTIFY,
      HandleSearchType::ByProtocol(_) => efi::BY_PROTOCOL,
    }
  }
}
