//! Native Keychain access. Secrets never pass through a helper process or argv.
use core_foundation::{array::CFArray, base::{CFType, TCFType}, data::CFData, dictionary::CFDictionary, string::CFString};
use core_foundation::base::{CFTypeRef, OSStatus};
use core_foundation::array::CFArrayRef;
use core_foundation::string::CFStringRef;
use core_foundation::{boolean::CFBoolean, date::CFDate};
use security_framework_sys::{item::{kSecClass, kSecClassGenericPassword, kSecAttrService, kSecAttrAccount, kSecValueData, kSecReturnData, kSecReturnAttributes, kSecUseAuthenticationUI}, keychain_item::{SecItemAdd, SecItemUpdate, SecItemCopyMatching}};
use std::ffi::c_char;

#[link(name = "Security", kind = "framework")]
extern "C" {
    static kSecAttrAccess: CFStringRef;
    static kSecAttrCreationDate: CFStringRef;
    static kSecAttrModificationDate: CFStringRef;
    static kSecUseAuthenticationUIFail: CFStringRef;
    fn SecTrustedApplicationCreateFromPath(path: *const c_char, application: *mut CFTypeRef) -> OSStatus;
    fn SecAccessCreate(description: CFStringRef, trusted: CFArrayRef, access: *mut CFTypeRef) -> OSStatus;
}

fn query(service: &str, account: &str) -> Vec<(CFString, CFType)> {
    unsafe {
        let key = |value| CFString::wrap_under_get_rule(value);
        vec![
            (key(kSecClass), key(kSecClassGenericPassword).into_CFType()),
            (key(kSecAttrService), CFString::new(service).into_CFType()),
            (key(kSecAttrAccount), CFString::new(account).into_CFType()),
        ]
    }
}

fn matching(query: &CFDictionary<CFString, CFType>) -> Result<Option<CFType>, String> {
    let mut value = std::ptr::null();
    let status = unsafe { SecItemCopyMatching(query.as_concrete_TypeRef(), &mut value) };
    if status == -25300 { return Ok(None); }
    if status != 0 {
        return Err(format!("credential_read_failed: macOS Keychain access failed (OSStatus {status})"));
    }
    if value.is_null() { return Err("credential_read_failed: Keychain returned no item".into()); }
    Ok(Some(unsafe { CFType::wrap_under_create_rule(value) }))
}

pub(super) fn password(service: &str, account: &str) -> Result<Option<Vec<u8>>, String> {
    let mut attributes = query(service, account);
    attributes.push((unsafe { CFString::wrap_under_get_rule(kSecReturnData) }, CFBoolean::true_value().into_CFType()));
    let Some(value) = matching(&CFDictionary::from_CFType_pairs(&attributes))? else { return Ok(None); };
    let data = value.downcast::<CFData>().ok_or("credential_read_failed: invalid Keychain data")?;
    Ok(Some(data.bytes().to_vec()))
}

/// Attribute-only lookup: it never requests password bytes or displays an auth UI.
/// A changed/deleted/recreated item must not reuse a cached credential.
pub(super) fn revision(service: &str, account: &str) -> Result<Option<(u64, u64)>, String> {
    let mut attributes = query(service, account);
    unsafe {
        attributes.push((CFString::wrap_under_get_rule(kSecReturnAttributes), CFBoolean::true_value().into_CFType()));
        attributes.push((CFString::wrap_under_get_rule(kSecUseAuthenticationUI), CFString::wrap_under_get_rule(kSecUseAuthenticationUIFail).into_CFType()));
    }
    let Some(value) = matching(&CFDictionary::from_CFType_pairs(&attributes))? else { return Ok(None); };
    let attributes = value.downcast::<CFDictionary>().ok_or("credential_read_failed: invalid Keychain attributes")?;
    let date = |key| -> Result<u64, String> {
        let value = attributes.find(unsafe { CFString::wrap_under_get_rule(key) }.as_CFTypeRef())
            .ok_or("credential_read_failed: Keychain item has no revision")?;
        let value = unsafe { CFType::wrap_under_get_rule(*value) };
        let date = value.downcast::<CFDate>().ok_or("credential_read_failed: invalid Keychain revision")?;
        Ok(date.abs_time().to_bits())
    };
    Ok(Some(unsafe { (date(kSecAttrCreationDate)?, date(kSecAttrModificationDate)?) }))
}

fn checked(status: OSStatus) -> Result<(), String> {
    if status == 0 { Ok(()) } else { Err(format!("macOS Keychain write failed (OSStatus {status})")) }
}

fn trusted_application(path: *const c_char) -> Result<CFType, String> {
    let mut value = std::ptr::null();
    // Security returns a retained object on success; the wrapper releases it.
    unsafe {
        checked(SecTrustedApplicationCreateFromPath(path, &mut value))?;
        if value.is_null() { return Err("macOS Keychain returned no trusted application".into()); }
        Ok(CFType::wrap_under_create_rule(value))
    }
}

pub(super) fn set_password(service: &str, account: &str, password: &[u8]) -> Result<(), String> {
    // Existing records retain their ACL. The only updated attribute is their data.
    unsafe {
        let key = |value| CFString::wrap_under_get_rule(value);
        let mut attributes = vec![
            (key(kSecClass), key(kSecClassGenericPassword).into_CFType()),
            (key(kSecAttrService), CFString::new(service).into_CFType()),
            (key(kSecAttrAccount), CFString::new(account).into_CFType()),
        ];
        let query = CFDictionary::from_CFType_pairs(&attributes);
        let data = CFData::from_buffer(password);
        let update = CFDictionary::from_CFType_pairs(&[(key(kSecValueData), data.clone().into_CFType())]);
        let status = SecItemUpdate(query.as_concrete_TypeRef(), update.as_concrete_TypeRef());
        const NOT_FOUND: OSStatus = -25300;
        const DUPLICATE: OSStatus = -25299;
        if status != NOT_FOUND { return checked(status); }

        // Preserve the existing product contract: Agent credential commands can
        // read through /usr/bin/security. Do not grant access to all applications.
        let current = trusted_application(std::ptr::null())?;
        let security = trusted_application(c"/usr/bin/security".as_ptr())?;
        let trusted = CFArray::from_CFTypes(&[current, security]);
        let description = CFString::new(service);
        let mut access = std::ptr::null();
        checked(SecAccessCreate(description.as_concrete_TypeRef(), trusted.as_concrete_TypeRef(), &mut access))?;
        if access.is_null() { return Err("macOS Keychain returned no access policy".into()); }
        let access = CFType::wrap_under_create_rule(access);
        attributes.push((key(kSecAttrAccess), access));
        attributes.push((key(kSecValueData), data.into_CFType()));
        let add = CFDictionary::from_CFType_pairs(&attributes);
        let status = SecItemAdd(add.as_concrete_TypeRef(), std::ptr::null_mut());
        if status == DUPLICATE {
            checked(SecItemUpdate(query.as_concrete_TypeRef(), update.as_concrete_TypeRef()))
        } else { checked(status) }
    }
}
