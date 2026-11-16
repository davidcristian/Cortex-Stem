//! Reading a string from a property store.

use windows::Win32::Foundation::PROPERTYKEY;
use windows::Win32::System::Com::StructuredStorage::PropVariantToStringAlloc;
use windows::Win32::UI::Shell::PropertiesSystem::IPropertyStore;

use super::{CoTaskWideString, PropVariantGuard};

/// A string property of a property store, trimmed. `None` when the key
/// is absent (`VT_EMPTY`), the value is blank, or it cannot be read as a
/// string. The conversion goes through `PropVariantToStringAlloc`, so
/// every caller (window property stores in `enumerate`, shortcut
/// property stores in `identity`) accepts the same variant types
/// (`VT_LPWSTR`, `VT_BSTR`, `VT_LPSTR`, ...). The caller must hold a COM
/// apartment on this thread.
pub fn property_store_string(store: &IPropertyStore, key: &PROPERTYKEY) -> Option<String> {
    // SAFETY: `key` is a valid PROPERTYKEY; the returned PROPVARIANT is
    // owned by us and cleared by the guard.
    let value = unsafe { store.GetValue(key) }.ok()?;
    let value = PropVariantGuard::new(value);
    // SAFETY: `value` is a live PROPVARIANT; the returned string is
    // CoTaskMem-allocated and freed by the guard.
    let text = unsafe { PropVariantToStringAlloc(value.as_ptr()) }.ok()?;
    let text = CoTaskWideString::new(text)?.to_string_lossy();
    let text = text.trim();
    (!text.is_empty()).then(|| text.to_owned())
}
