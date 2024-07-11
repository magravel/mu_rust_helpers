use core::{
  ffi::c_void,
  ops::{BitOr, BitOrAssign},
  ptr, slice,
};

use r_efi::efi;

use crate::StandardBootServices;

pub trait MemoryAllocationServices {
  fn allocate_pages(
    &self,
    alloc_type: AllocType,
    memory_type: MemoryType,
    nb_pages: usize,
  ) -> Result<usize, efi::Status>;

  fn free_pages(&self, address: usize, nb_pages: usize) -> Result<(), efi::Status>;

  fn get_memory_map<'a>(&self, buffer: &'a mut [u8]) -> Result<MemoryMap<'a>, (efi::Status, usize)>;
  fn allocate_pool(&self, memory_type: MemoryType, size: usize) -> Result<*mut u8, efi::Status>;
  fn free_pool(&self, buffer: *mut u8) -> Result<(), efi::Status>;
}

impl MemoryAllocationServices for StandardBootServices<'_> {
  fn allocate_pages(
    &self,
    alloc_type: AllocType,
    memory_type: MemoryType,
    nb_pages: usize,
  ) -> Result<usize, efi::Status> {
    let mut memory_address = match alloc_type {
      AllocType::Address(address) => address,
      _ => 0,
    };
    match (self.efi_boot_services().allocate_pages)(
      alloc_type.into(),
      memory_type.into(),
      nb_pages,
      ptr::addr_of_mut!(memory_address) as *mut u64,
    ) {
      s if s.is_error() => Err(s),
      _ => Ok(memory_address),
    }
  }

  fn free_pages(&self, address: usize, nb_pages: usize) -> Result<(), efi::Status> {
    match (self.efi_boot_services().free_pages)(address as u64, nb_pages) {
      s if s.is_error() => Err(s),
      _ => Ok(()),
    }
  }

  fn get_memory_map<'a>(&self, buffer: &'a mut [u8]) -> Result<MemoryMap<'a>, (efi::Status, usize)> {
    let mut memory_map_size = buffer.len();
    let mut map_key = 0;
    let mut descriptor_size = 0;
    let mut descriptor_version = 0;
    match (self.efi_boot_services().get_memory_map)(
      ptr::addr_of_mut!(memory_map_size),
      buffer.as_mut_ptr() as *mut _,
      ptr::addr_of_mut!(map_key),
      ptr::addr_of_mut!(descriptor_size),
      ptr::addr_of_mut!(descriptor_version),
    ) {
      s if s == efi::Status::BUFFER_TOO_SMALL => return Err((s, memory_map_size)),
      s if s.is_error() => return Err((s, 0)),
      _ => (),
    }
    Ok(MemoryMap {
      descriptors: unsafe { slice::from_raw_parts(buffer.as_ptr() as *const _, memory_map_size / descriptor_size) },
      map_key,
      descriptor_version,
    })
  }

  fn allocate_pool(&self, memory_type: MemoryType, size: usize) -> Result<*mut u8, efi::Status> {
    let mut buffer = ptr::null_mut();
    match (self.efi_boot_services().allocate_pool)(memory_type.into(), size, ptr::addr_of_mut!(buffer)) {
      s if s.is_error() => return Err(s),
      _ => Ok(buffer as *mut u8),
    }
  }

  fn free_pool(&self, buffer: *mut u8) -> Result<(), efi::Status> {
    match (self.efi_boot_services().free_pool)(buffer as *mut c_void) {
      s if s.is_error() => return Err(s),
      _ => Ok(()),
    }
  }
}

#[derive(Debug)]
pub enum AllocType {
  AnyPage,
  MaxAddress,
  Address(usize),
}

#[derive(Debug)]
pub enum MemoryType {
  ReservedMemoryType,
  LoaderCode,
  LoaderData,
  BootServicesCode,
  BootServicesData,
  RuntimeServicesCode,
  RuntimeServicesData,
  ConventionalMemory,
  UnusableMemory,
  ACPIReclaimMemory,
  ACPIMemoryNVS,
  MemoryMappedIO,
  MemoryMappedIOPortSpace,
  PalCode,
  PersistentMemory,
  UnacceptedMemoryType,
}

#[derive(Debug)]
pub struct MemoryMap<'a> {
  pub descriptors: &'a [MemoryDescriptor],
  pub map_key: usize,
  pub descriptor_version: u32,
}

#[derive(Debug)]
pub struct MemoryDescriptor {
  pub memory_type: MemoryType,
  pub physical_start: usize,
  pub virtual_start: usize,
  pub nb_pages: usize,
  pub attribute: MemroyAttribute,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MemroyAttribute(u64);

impl MemroyAttribute {
  pub const UC: MemroyAttribute = MemroyAttribute(efi::MEMORY_UC);
  pub const WC: MemroyAttribute = MemroyAttribute(efi::MEMORY_WC);
  pub const WT: MemroyAttribute = MemroyAttribute(efi::MEMORY_WT);
  pub const WB: MemroyAttribute = MemroyAttribute(efi::MEMORY_WB);
  pub const UCE: MemroyAttribute = MemroyAttribute(efi::MEMORY_UCE);
  pub const WP: MemroyAttribute = MemroyAttribute(efi::MEMORY_WP);
  pub const RP: MemroyAttribute = MemroyAttribute(efi::MEMORY_RP);
  pub const XP: MemroyAttribute = MemroyAttribute(efi::MEMORY_XP);
  pub const NV: MemroyAttribute = MemroyAttribute(efi::MEMORY_NV);
  pub const MORE_RELIABLE: MemroyAttribute = MemroyAttribute(efi::MEMORY_MORE_RELIABLE);
  pub const RO: MemroyAttribute = MemroyAttribute(efi::MEMORY_RO);
  pub const SP: MemroyAttribute = MemroyAttribute(efi::MEMORY_SP);
  pub const CPU_CRYPTO: MemroyAttribute = MemroyAttribute(efi::MEMORY_CPU_CRYPTO);
  pub const RUNTIME: MemroyAttribute = MemroyAttribute(efi::MEMORY_RUNTIME);
  pub const ISA_VALID: MemroyAttribute = MemroyAttribute(efi::MEMORY_ISA_VALID);
  pub const ISA_MASK: MemroyAttribute = MemroyAttribute(efi::MEMORY_ISA_MASK);
}

impl BitOr for MemroyAttribute {
  type Output = MemroyAttribute;

  fn bitor(self, rhs: Self) -> Self::Output {
    MemroyAttribute(self.0 | rhs.0)
  }
}

impl BitOrAssign for MemroyAttribute {
  fn bitor_assign(&mut self, rhs: Self) {
    self.0 |= rhs.0
  }
}

impl Into<efi::AllocateType> for AllocType {
  fn into(self) -> efi::AllocateType {
    match self {
      AllocType::AnyPage => efi::ALLOCATE_ANY_PAGES,
      AllocType::MaxAddress => efi::ALLOCATE_MAX_ADDRESS,
      AllocType::Address(_) => efi::ALLOCATE_ADDRESS,
    }
  }
}

impl Into<efi::MemoryType> for MemoryType {
  fn into(self) -> efi::MemoryType {
    match self {
      Self::ReservedMemoryType => efi::RESERVED_MEMORY_TYPE,
      Self::LoaderCode => efi::LOADER_CODE,
      Self::LoaderData => efi::LOADER_DATA,
      Self::BootServicesCode => efi::BOOT_SERVICES_CODE,
      Self::BootServicesData => efi::BOOT_SERVICES_DATA,
      Self::RuntimeServicesCode => efi::RUNTIME_SERVICES_CODE,
      Self::RuntimeServicesData => efi::RUNTIME_SERVICES_DATA,
      Self::ConventionalMemory => efi::CONVENTIONAL_MEMORY,
      Self::UnusableMemory => efi::UNUSABLE_MEMORY,
      Self::ACPIReclaimMemory => efi::ACPI_RECLAIM_MEMORY,
      Self::ACPIMemoryNVS => efi::ACPI_MEMORY_NVS,
      Self::MemoryMappedIO => efi::MEMORY_MAPPED_IO,
      Self::MemoryMappedIOPortSpace => efi::MEMORY_MAPPED_IO_PORT_SPACE,
      Self::PalCode => efi::PAL_CODE,
      Self::PersistentMemory => efi::PERSISTENT_MEMORY,
      Self::UnacceptedMemoryType => efi::UNACCEPTED_MEMORY_TYPE,
    }
  }
}

impl Into<u64> for MemroyAttribute {
  fn into(self) -> u64 {
    self.0
  }
}
