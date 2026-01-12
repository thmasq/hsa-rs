#![allow(clippy::used_underscore_items, clippy::cast_possible_truncation)]

const _IOC_NRBITS: u32 = 8;
const _IOC_TYPEBITS: u32 = 8;
const _IOC_SIZEBITS: u32 = 14;
const _IOC_DIRBITS: u32 = 2;

const _IOC_NRSHIFT: u32 = 0;
const _IOC_TYPESHIFT: u32 = _IOC_NRSHIFT + _IOC_NRBITS;
const _IOC_SIZESHIFT: u32 = _IOC_TYPESHIFT + _IOC_TYPEBITS;
const _IOC_DIRSHIFT: u32 = _IOC_SIZESHIFT + _IOC_SIZEBITS;

const _IOC_NONE: u32 = 0;
const _IOC_WRITE: u32 = 1;
const _IOC_READ: u32 = 2;

#[must_use]
pub const fn _ioc(dir: u32, type_: u32, nr: u32, size: usize) -> u32 {
    (dir << _IOC_DIRSHIFT)
        | (type_ << _IOC_TYPESHIFT)
        | (nr << _IOC_NRSHIFT)
        | ((size as u32) << _IOC_SIZESHIFT)
}

#[must_use]
pub const fn io(type_: u32, nr: u32) -> u32 {
    _ioc(_IOC_NONE, type_, nr, 0)
}

#[must_use]
pub const fn ior<T>(type_: u32, nr: u32) -> u32 {
    _ioc(_IOC_READ, type_, nr, std::mem::size_of::<T>())
}

#[must_use]
pub const fn iow<T>(type_: u32, nr: u32) -> u32 {
    _ioc(_IOC_WRITE, type_, nr, std::mem::size_of::<T>())
}

#[must_use]
pub const fn iowr<T>(type_: u32, nr: u32) -> u32 {
    _ioc(_IOC_READ | _IOC_WRITE, type_, nr, std::mem::size_of::<T>())
}
