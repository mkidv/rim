// SPDX-License-Identifier: MIT

//! Subpath confinement wrapper around filesystem tree injectors.

use rimfs_core::allocator::FsHandle;
use rimfs_core::errors::FsInjectorResult;
use rimfs_core::injector::FsTreeInjector;
use rimfs_core::resolver::attr::FileAttributes;
use rimio::RimRead;

/// Wraps an underlying [FsTreeInjector] to direct all operations into a destination subpath.
pub struct SubpathInjector<'a, H: FsHandle, I: FsTreeInjector<H> + ?Sized> {
    inner: &'a mut I,
    components: Vec<String>,
    _phantom: core::marker::PhantomData<H>,
}

impl<'a, H: FsHandle, I: FsTreeInjector<H> + ?Sized> SubpathInjector<'a, H, I> {
    pub fn new(inner: &'a mut I, subpath: &str) -> Self {
        let components = subpath
            .split('/')
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string())
            .collect();
        Self {
            inner,
            components,
            _phantom: core::marker::PhantomData,
        }
    }
}

impl<'a, H: FsHandle, I: FsTreeInjector<H> + ?Sized> FsTreeInjector<H>
    for SubpathInjector<'a, H, I>
{
    fn set_root_context(&mut self, attr: &FileAttributes) -> FsInjectorResult {
        self.inner.set_root_context(attr)?;
        for comp in &self.components {
            let dir_attr = FileAttributes::new_dir();
            self.inner.write_dir(comp, &dir_attr)?;
        }
        Ok(())
    }

    fn write_dir(&mut self, name: &str, attr: &FileAttributes) -> FsInjectorResult {
        self.inner.write_dir(name, attr)
    }

    fn write_file(
        &mut self,
        name: &str,
        source: &mut dyn RimRead,
        size: u64,
        attr: &FileAttributes,
    ) -> FsInjectorResult {
        self.inner.write_file(name, source, size, attr)
    }

    fn write_symlink(
        &mut self,
        name: &str,
        target: &str,
        attr: &FileAttributes,
    ) -> FsInjectorResult {
        self.inner.write_symlink(name, target, attr)
    }

    fn flush_current(&mut self) -> FsInjectorResult {
        self.inner.flush_current()
    }

    fn flush(&mut self) -> FsInjectorResult {
        for _ in &self.components {
            self.inner.flush_current()?;
        }
        self.inner.flush()
    }
}
