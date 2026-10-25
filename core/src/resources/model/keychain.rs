//! Native Keychain access. Secrets never pass through a helper process or argv.
use core_foundation::{array::CFArray, base::{CFType, TCFType}, data::CFData, dictionary::CFDictionary, string::CFString};
use core_foundation::base::{CFTypeRef, OSStatus};
use core_foundation::array::CFArrayRef;
use core_foundation::string::CFStringRef;
use core_foundation::{boolean::CFBoolean, date::CFDate};
use security_framework_sys::{item::{kSecClass, kSecClassGenericPassword, kSecAttrService, kSecAttrAccount, kSecAttrLabel, kSecValueData, kSecReturnData, kSecReturnAttributes, kSecReturnRef, kSecUseAuthenticationUI}, keychain_item::{SecItemAdd, SecItemUpdate, SecItemCopyMatching}};
use std::ffi::c_char;

#[link(name = "Security", kind = "framework")]
extern "C" {
    static kSecAttrAccess: CFStringRef;
    static kSecAttrCreationDate: CFStringRef;
    static kSecAttrModificationDate: CFStringRef;
    static kSecUseAuthenticationUIFail: CFStringRef;
    fn SecTrustedApplicationCreateFromPath(path: *const c_char, application: *mut CFTypeRef) -> OSStatus;
    fn SecAccessCreate(description: CFStringRef, trusted: CFArrayRef, access: *mut CFTypeRef) -> OSStatus;
    fn SecKeychainItemCopyAccess(item: CFTypeRef, access: *mut CFTypeRef) -> OSStatus;
    fn SecAccessCopyACLList(access: CFTypeRef, list: *mut CFArrayRef) -> OSStatus;
    fn SecACLCopyContents(acl: CFTypeRef, applications: *mut CFArrayRef, description: *mut CFStringRef, prompt: *mut u16) -> OSStatus;
    fn SecACLSetContents(acl: CFTypeRef, applications: CFArrayRef, description: CFStringRef, prompt: u16) -> OSStatus;
}

/// The service is a durable lookup key also embedded in existing Agent commands.
/// Branding belongs in the label and ACL prompt description, not that identifier.
fn display_name(service: &str) -> Option<String> {
    service.strip_prefix("com.scoheart.mux.model-provider.")
        .filter(|id| !id.is_empty())
        .map(|id| format!("Arca · Model Provider · {id}"))
        .or_else(|| service.strip_prefix("com.scoheart.mux.model-profile.")
            .filter(|id| !id.is_empty())
            .map(|id| format!("Arca · Model Profile · {id}")))
}

/// Only replace our former human-readable service descriptions. Special ACLs
/// (including partition IDs), app lists, authorizations and prompt flags survive.
fn rename_access_descriptions(access: &CFType, service: &str, name: &CFString) -> Result<bool, String> {
    let mut list = std::ptr::null();
    unsafe { checked(SecAccessCopyACLList(access.as_CFTypeRef(), &mut list))?; }
    if list.is_null() { return Err("macOS Keychain returned no access list".into()); }
    let list = unsafe { CFArray::<CFType>::wrap_under_create_rule(list) };
    let mut changed = false;
    for acl in list.iter() {
        let mut apps = std::ptr::null();
        let mut description = std::ptr::null();
        let mut prompt = 0;
        let status = unsafe { SecACLCopyContents(acl.as_CFTypeRef(), &mut apps, &mut description, &mut prompt) };
        // Copy results are retained. Keep nil distinct from an empty app list:
        // nil means every application is trusted, empty means none is trusted.
        let apps = (!apps.is_null()).then(|| unsafe { CFArray::<CFType>::wrap_under_create_rule(apps) });
        let description = (!description.is_null()).then(|| unsafe { CFString::wrap_under_create_rule(description) });
        checked(status)?;
        if description.as_ref().is_some_and(|value| value.to_string() == service) {
            unsafe { checked(SecACLSetContents(acl.as_CFTypeRef(),
                apps.as_ref().map_or(std::ptr::null(), |apps| apps.as_concrete_TypeRef()),
                name.as_concrete_TypeRef(), prompt))?; }
            changed = true;
        }
    }
    Ok(changed)
}

/// Best-effort metadata upgrade after an authorized read/write. Never open an
/// extra password dialog just to rename an item, and never copy its secret.
pub(super) fn refresh_display_name(service: &str, account: &str) -> Result<(), String> {
    let Some(name) = display_name(service) else { return Ok(()); };
    let name = CFString::new(&name);
    let mut attributes = query(service, account);
    unsafe {
        attributes.push((CFString::wrap_under_get_rule(kSecUseAuthenticationUI), CFString::wrap_under_get_rule(kSecUseAuthenticationUIFail).into_CFType()));
        attributes.push((CFString::wrap_under_get_rule(kSecReturnRef), CFBoolean::true_value().into_CFType()));
    }
    let Some(item) = matching(&CFDictionary::from_CFType_pairs(&attributes))? else { return Ok(()); };
    let mut access = std::ptr::null();
    unsafe { checked(SecKeychainItemCopyAccess(item.as_CFTypeRef(), &mut access))?; }
    if access.is_null() { return Err("macOS Keychain returned no access policy".into()); }
    let access = unsafe { CFType::wrap_under_create_rule(access) };
    let changed = rename_access_descriptions(&access, service, &name)?;

    attributes.pop(); // Remove return-ref; this is an update query now.
    let mut metadata_query = attributes.clone();
    metadata_query.push((unsafe { CFString::wrap_under_get_rule(kSecReturnAttributes) }, CFBoolean::true_value().into_CFType()));
    let Some(metadata) = matching(&CFDictionary::from_CFType_pairs(&metadata_query))? else { return Ok(()); };
    let metadata = metadata.downcast::<CFDictionary>().ok_or("macOS Keychain returned invalid attributes")?;
    let label = metadata.find(unsafe { CFString::wrap_under_get_rule(kSecAttrLabel) }.as_CFTypeRef())
        .and_then(|value| unsafe { CFType::wrap_under_get_rule(*value) }.downcast::<CFString>());
    let mut updates = Vec::new();
    if label.as_ref().is_none_or(|value| value.to_string() == service) {
        updates.push((unsafe { CFString::wrap_under_get_rule(kSecAttrLabel) }, name.into_CFType()));
    }
    if changed {
        updates.push((unsafe { CFString::wrap_under_get_rule(kSecAttrAccess) }, access));
    }
    if updates.is_empty() { return Ok(()); }
    unsafe { checked(SecItemUpdate(CFDictionary::from_CFType_pairs(&attributes).as_concrete_TypeRef(),
        CFDictionary::from_CFType_pairs(&updates).as_concrete_TypeRef())) }
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
    password_with_ui(service, account, true)
}

pub(super) fn password_with_ui(service: &str, account: &str, interactive: bool) -> Result<Option<Vec<u8>>, String> {
    let mut attributes = query(service, account);
    if !interactive {
        attributes.push((unsafe { CFString::wrap_under_get_rule(kSecUseAuthenticationUI) }, unsafe { CFString::wrap_under_get_rule(kSecUseAuthenticationUIFail) }.into_CFType()));
    }
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
    write_password(service, account, password, false, true)
}

pub(super) fn set_vault_password(service: &str, account: &str, password: &[u8]) -> Result<(), String> {
    write_password(service, account, password, true, false)
}

fn write_password(service: &str, account: &str, password: &[u8], vault: bool, interactive: bool) -> Result<(), String> {
    // Existing records retain their access rules; only branding metadata changes.
    unsafe {
        let key = |value| CFString::wrap_under_get_rule(value);
        let mut attributes = vec![
            (key(kSecClass), key(kSecClassGenericPassword).into_CFType()),
            (key(kSecAttrService), CFString::new(service).into_CFType()),
            (key(kSecAttrAccount), CFString::new(account).into_CFType()),
        ];
        if !interactive {
            attributes.push((key(kSecUseAuthenticationUI), key(kSecUseAuthenticationUIFail).into_CFType()));
        }
        let query = CFDictionary::from_CFType_pairs(&attributes);
        let data = CFData::from_buffer(password);
        let update = CFDictionary::from_CFType_pairs(&[(key(kSecValueData), data.clone().into_CFType())]);
        let status = SecItemUpdate(query.as_concrete_TypeRef(), update.as_concrete_TypeRef());
        const NOT_FOUND: OSStatus = -25300;
        const DUPLICATE: OSStatus = -25299;
        if status != NOT_FOUND {
            checked(status)?;
            let _ = refresh_display_name(service, account);
            return Ok(());
        }

        // Preserve the existing product contract: Agent credential commands can
        // read through /usr/bin/security. Do not grant access to all applications.
        let current = trusted_application(std::ptr::null())?;
        let mut applications = vec![current];
        if !vault { applications.push(trusted_application(c"/usr/bin/security".as_ptr())?); }
        let trusted = CFArray::from_CFTypes(&applications);
        let description = CFString::new(&if vault { "Arca · Credentials".into() }
            else { display_name(service).unwrap_or_else(|| service.into()) });
        let mut access = std::ptr::null();
        checked(SecAccessCreate(description.as_concrete_TypeRef(), trusted.as_concrete_TypeRef(), &mut access))?;
        if access.is_null() { return Err("macOS Keychain returned no access policy".into()); }
        let access = CFType::wrap_under_create_rule(access);
        attributes.push((key(kSecAttrLabel), description.into_CFType()));
        attributes.push((key(kSecAttrAccess), access));
        attributes.push((key(kSecValueData), data.into_CFType()));
        let add = CFDictionary::from_CFType_pairs(&attributes);
        let status = SecItemAdd(add.as_concrete_TypeRef(), std::ptr::null_mut());
        if status == DUPLICATE {
            checked(SecItemUpdate(query.as_concrete_TypeRef(), update.as_concrete_TypeRef()))?;
            let _ = refresh_display_name(service, account);
            Ok(())
        } else { checked(status) }
    }
}
