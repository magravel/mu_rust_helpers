use r_efi::efi;

#[derive(Debug, PartialEq, Eq, Clone, Copy, PartialOrd, Ord)]
#[repr(transparent)]
pub struct Tpl(pub usize);

impl Tpl {
  pub const APPLICATION: Tpl = Tpl(efi::TPL_APPLICATION);
  pub const CALLBACK: Tpl = Tpl(efi::TPL_CALLBACK);
  pub const NOTIFY: Tpl = Tpl(efi::TPL_NOTIFY);
  pub const HIGH_LEVEL: Tpl = Tpl(efi::TPL_HIGH_LEVEL);
}

impl Into<usize> for Tpl {
  fn into(self) -> usize {
    self.0
  }
}

impl Into<Tpl> for usize {
  fn into(self) -> Tpl {
    Tpl(self)
  }
}

#[cfg(test)]
mod test {
  use super::*;

  #[test]
  fn t() {
    fn foo(tpl: efi::Tpl) {
      println!("{tpl:?}")
    }
    let tpl = Tpl::APPLICATION;
    foo(tpl.into());
  }
}
