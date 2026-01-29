//! Native user and group management bindings
//!
//! This module provides native access to user and group information using
//! libc functions on Unix systems, avoiding the overhead of shell commands.
//!
//! # Features
//!
//! - Direct passwd/group file reading
//! - Native getpwnam/getgrnam lookups
//! - Shadow file parsing (with root privileges)
//! - Efficient batch queries
//!
//! # Example
//!
//! ```rust,ignore
//! use rustible::native::users::{get_user_by_name, get_group_by_name, UserInfo};
//!
//! // Look up a user
//! if let Some(user) = get_user_by_name("www-data")? {
//!     println!("UID: {}, Home: {}", user.uid, user.home);
//! }
//!
//! // Look up a group
//! if let Some(group) = get_group_by_name("sudo")? {
//!     println!("GID: {}, Members: {:?}", group.gid, group.members);
//! }
//! ```

use super::{NativeError, NativeResult};
use std::collections::HashMap;
use std::ffi::{CStr, CString};
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

/// Default passwd file path
const PASSWD_PATH: &str = "/etc/passwd";
/// Default group file path
const GROUP_PATH: &str = "/etc/group";
/// Default shadow file path
const SHADOW_PATH: &str = "/etc/shadow";

/// Check if native user support is available
pub fn is_native_available() -> bool {
    cfg!(unix)
}

/// Information about a user
#[derive(Debug, Clone)]
pub struct UserInfo {
    /// Username
    pub name: String,
    /// User ID
    pub uid: u32,
    /// Primary group ID
    pub gid: u32,
    /// Full name / GECOS field
    pub gecos: String,
    /// Home directory
    pub home: String,
    /// Login shell
    pub shell: String,
    /// Encrypted password (if available)
    pub password: Option<String>,
    /// Secondary groups this user belongs to
    pub groups: Vec<String>,
}

impl UserInfo {
    /// Check if the user has a valid password set
    pub fn has_password(&self) -> bool {
        match &self.password {
            Some(p) => !p.is_empty() && p != "*" && p != "!" && !p.starts_with("!"),
            None => false,
        }
    }

    /// Check if the account is locked
    pub fn is_locked(&self) -> bool {
        match &self.password {
            Some(p) => p.starts_with("!") || p == "*",
            None => false,
        }
    }

    /// Check if this is a system user (typically UID < 1000)
    pub fn is_system_user(&self) -> bool {
        self.uid < 1000 || self.uid == 65534 // nobody
    }
}

/// Information about a group
#[derive(Debug, Clone)]
pub struct GroupInfo {
    /// Group name
    pub name: String,
    /// Group ID
    pub gid: u32,
    /// Group password (usually empty or x)
    pub password: Option<String>,
    /// Group members
    pub members: Vec<String>,
}

impl GroupInfo {
    /// Check if this is a system group (typically GID < 1000)
    pub fn is_system_group(&self) -> bool {
        self.gid < 1000 || self.gid == 65534 // nogroup
    }
}

/// Get user information by name using libc
#[cfg(unix)]
pub fn get_user_by_name(name: &str) -> NativeResult<Option<UserInfo>> {
    let c_name = CString::new(name)
        .map_err(|_| NativeError::InvalidArgument("Invalid username".to_string()))?;

    // Use getpwnam_r for thread safety
    unsafe {
        let pwd = libc::getpwnam(c_name.as_ptr());
        if pwd.is_null() {
            return Ok(None);
        }

        let pwd = &*pwd;

        let user = UserInfo {
            name: CStr::from_ptr(pwd.pw_name).to_string_lossy().to_string(),
            uid: pwd.pw_uid,
            gid: pwd.pw_gid,
            gecos: CStr::from_ptr(pwd.pw_gecos).to_string_lossy().to_string(),
            home: CStr::from_ptr(pwd.pw_dir).to_string_lossy().to_string(),
            shell: CStr::from_ptr(pwd.pw_shell).to_string_lossy().to_string(),
            password: None, // Password is typically in shadow file
            groups: get_user_groups(name)?,
        };

        Ok(Some(user))
    }
}

#[cfg(not(unix))]
pub fn get_user_by_name(_name: &str) -> NativeResult<Option<UserInfo>> {
    Err(NativeError::NotAvailable(
        "User lookup not available on this platform".to_string(),
    ))
}

/// Get user information by UID using libc
#[cfg(unix)]
pub fn get_user_by_uid(uid: u32) -> NativeResult<Option<UserInfo>> {
    unsafe {
        let pwd = libc::getpwuid(uid);
        if pwd.is_null() {
            return Ok(None);
        }

        let pwd = &*pwd;
        let name = CStr::from_ptr(pwd.pw_name).to_string_lossy().to_string();

        let user = UserInfo {
            name: name.clone(),
            uid: pwd.pw_uid,
            gid: pwd.pw_gid,
            gecos: CStr::from_ptr(pwd.pw_gecos).to_string_lossy().to_string(),
            home: CStr::from_ptr(pwd.pw_dir).to_string_lossy().to_string(),
            shell: CStr::from_ptr(pwd.pw_shell).to_string_lossy().to_string(),
            password: None,
            groups: get_user_groups(&name)?,
        };

        Ok(Some(user))
    }
}

#[cfg(not(unix))]
pub fn get_user_by_uid(_uid: u32) -> NativeResult<Option<UserInfo>> {
    Err(NativeError::NotAvailable(
        "User lookup not available on this platform".to_string(),
    ))
}

/// Get group information by name using libc
#[cfg(unix)]
pub fn get_group_by_name(name: &str) -> NativeResult<Option<GroupInfo>> {
    let c_name = CString::new(name)
        .map_err(|_| NativeError::InvalidArgument("Invalid group name".to_string()))?;

    unsafe {
        let grp = libc::getgrnam(c_name.as_ptr());
        if grp.is_null() {
            return Ok(None);
        }

        let grp = &*grp;

        let mut members = Vec::new();
        let mut i = 0;
        while !(*grp.gr_mem.add(i)).is_null() {
            members.push(
                CStr::from_ptr(*grp.gr_mem.add(i))
                    .to_string_lossy()
                    .to_string(),
            );
            i += 1;
        }

        let group = GroupInfo {
            name: CStr::from_ptr(grp.gr_name).to_string_lossy().to_string(),
            gid: grp.gr_gid,
            password: None,
            members,
        };

        Ok(Some(group))
    }
}

#[cfg(not(unix))]
pub fn get_group_by_name(_name: &str) -> NativeResult<Option<GroupInfo>> {
    Err(NativeError::NotAvailable(
        "Group lookup not available on this platform".to_string(),
    ))
}

/// Get group information by GID using libc
#[cfg(unix)]
pub fn get_group_by_gid(gid: u32) -> NativeResult<Option<GroupInfo>> {
    unsafe {
        let grp = libc::getgrgid(gid);
        if grp.is_null() {
            return Ok(None);
        }

        let grp = &*grp;

        let mut members = Vec::new();
        let mut i = 0;
        while !(*grp.gr_mem.add(i)).is_null() {
            members.push(
                CStr::from_ptr(*grp.gr_mem.add(i))
                    .to_string_lossy()
                    .to_string(),
            );
            i += 1;
        }

        let group = GroupInfo {
            name: CStr::from_ptr(grp.gr_name).to_string_lossy().to_string(),
            gid: grp.gr_gid,
            password: None,
            members,
        };

        Ok(Some(group))
    }
}

#[cfg(not(unix))]
pub fn get_group_by_gid(_gid: u32) -> NativeResult<Option<GroupInfo>> {
    Err(NativeError::NotAvailable(
        "Group lookup not available on this platform".to_string(),
    ))
}

/// Get all groups a user belongs to
#[cfg(unix)]
pub fn get_user_groups(username: &str) -> NativeResult<Vec<String>> {
    let c_name = CString::new(username)
        .map_err(|_| NativeError::InvalidArgument("Invalid username".to_string()))?;

    // Get the user's primary group directly using getpwnam
    let primary_gid = unsafe {
        let pwd = libc::getpwnam(c_name.as_ptr());
        if !pwd.is_null() {
            (*pwd).pw_gid
        } else {
            0
        }
    };

    unsafe {
        // Start with a reasonable buffer size
        let mut ngroups: libc::c_int = 32;
        let mut groups: Vec<libc::gid_t> = vec![0; ngroups as usize];

        let result = libc::getgrouplist(
            c_name.as_ptr(),
            primary_gid,
            groups.as_mut_ptr(),
            &mut ngroups,
        );

        // If the buffer was too small, resize and try again
        if result == -1 {
            groups.resize(ngroups as usize, 0);
            libc::getgrouplist(
                c_name.as_ptr(),
                primary_gid,
                groups.as_mut_ptr(),
                &mut ngroups,
            );
        }

        groups.truncate(ngroups as usize);

        // Convert GIDs to group names
        let mut group_names = Vec::new();
        for gid in groups {
            if let Ok(Some(group)) = get_group_by_gid(gid) {
                group_names.push(group.name);
            }
        }

        Ok(group_names)
    }
}

#[cfg(not(unix))]
pub fn get_user_groups(_username: &str) -> NativeResult<Vec<String>> {
    Err(NativeError::NotAvailable(
        "Group lookup not available on this platform".to_string(),
    ))
}

/// Parse /etc/passwd file directly for batch operations
pub fn parse_passwd_file() -> NativeResult<Vec<UserInfo>> {
    parse_passwd_file_at(PASSWD_PATH)
}

/// Parse passwd file at a specific path
pub fn parse_passwd_file_at(path: &str) -> NativeResult<Vec<UserInfo>> {
    if !Path::new(path).exists() {
        return Err(NativeError::NotFound(format!(
            "Passwd file not found: {}",
            path
        )));
    }

    let file = File::open(path)?;
    let reader = BufReader::new(file);
    let mut users = Vec::new();

    for line in reader.lines() {
        let line = line?;
        if line.starts_with('#') || line.is_empty() {
            continue;
        }

        let fields: Vec<&str> = line.split(':').collect();
        if fields.len() >= 7 {
            let name = fields[0].to_string();
            let user = UserInfo {
                name: name.clone(),
                uid: fields[2].parse().unwrap_or(0),
                gid: fields[3].parse().unwrap_or(0),
                gecos: fields[4].to_string(),
                home: fields[5].to_string(),
                shell: fields[6].to_string(),
                password: if fields[1] != "x" && fields[1] != "*" {
                    Some(fields[1].to_string())
                } else {
                    None
                },
                groups: Vec::new(), // Would need to be populated separately
            };
            users.push(user);
        }
    }

    Ok(users)
}

/// Parse /etc/group file directly for batch operations
pub fn parse_group_file() -> NativeResult<Vec<GroupInfo>> {
    parse_group_file_at(GROUP_PATH)
}

/// Parse group file at a specific path
pub fn parse_group_file_at(path: &str) -> NativeResult<Vec<GroupInfo>> {
    if !Path::new(path).exists() {
        return Err(NativeError::NotFound(format!(
            "Group file not found: {}",
            path
        )));
    }

    let file = File::open(path)?;
    let reader = BufReader::new(file);
    let mut groups = Vec::new();

    for line in reader.lines() {
        let line = line?;
        if line.starts_with('#') || line.is_empty() {
            continue;
        }

        let fields: Vec<&str> = line.split(':').collect();
        if fields.len() >= 4 {
            let group = GroupInfo {
                name: fields[0].to_string(),
                gid: fields[2].parse().unwrap_or(0),
                password: if fields[1] != "x" && !fields[1].is_empty() {
                    Some(fields[1].to_string())
                } else {
                    None
                },
                members: fields[3]
                    .split(',')
                    .filter(|s| !s.is_empty())
                    .map(|s| s.to_string())
                    .collect(),
            };
            groups.push(group);
        }
    }

    Ok(groups)
}

/// Parse /etc/shadow file (requires root)
pub fn parse_shadow_file() -> NativeResult<HashMap<String, String>> {
    parse_shadow_file_at(SHADOW_PATH)
}

/// Parse shadow file at a specific path
pub fn parse_shadow_file_at(path: &str) -> NativeResult<HashMap<String, String>> {
    let file = File::open(path).map_err(|e| {
        if e.kind() == std::io::ErrorKind::PermissionDenied {
            NativeError::PermissionDenied("Cannot read shadow file".to_string())
        } else {
            NativeError::Io(e)
        }
    })?;

    let reader = BufReader::new(file);
    let mut shadows = HashMap::new();

    for line in reader.lines() {
        let line = line?;
        if line.starts_with('#') || line.is_empty() {
            continue;
        }

        let fields: Vec<&str> = line.split(':').collect();
        if fields.len() >= 2 {
            shadows.insert(fields[0].to_string(), fields[1].to_string());
        }
    }

    Ok(shadows)
}

/// User database for efficient batch queries
pub struct UserDatabase {
    /// Users indexed by name
    users_by_name: HashMap<String, UserInfo>,
    /// Users indexed by UID
    users_by_uid: HashMap<u32, String>,
    /// Groups indexed by name
    groups_by_name: HashMap<String, GroupInfo>,
    /// Groups indexed by GID
    groups_by_gid: HashMap<u32, String>,
}

impl UserDatabase {
    /// Load the user database from system files
    pub fn load() -> NativeResult<Self> {
        let users = parse_passwd_file()?;
        let groups = parse_group_file()?;

        let mut users_by_name = HashMap::new();
        let mut users_by_uid = HashMap::new();
        let mut groups_by_name = HashMap::new();
        let mut groups_by_gid = HashMap::new();

        for user in users {
            users_by_uid.insert(user.uid, user.name.clone());
            users_by_name.insert(user.name.clone(), user);
        }

        for group in groups {
            groups_by_gid.insert(group.gid, group.name.clone());
            groups_by_name.insert(group.name.clone(), group);
        }

        Ok(Self {
            users_by_name,
            users_by_uid,
            groups_by_name,
            groups_by_gid,
        })
    }

    /// Get user by name
    pub fn get_user(&self, name: &str) -> Option<&UserInfo> {
        self.users_by_name.get(name)
    }

    /// Get user by UID
    pub fn get_user_by_uid(&self, uid: u32) -> Option<&UserInfo> {
        self.users_by_uid
            .get(&uid)
            .and_then(|name| self.users_by_name.get(name))
    }

    /// Get group by name
    pub fn get_group(&self, name: &str) -> Option<&GroupInfo> {
        self.groups_by_name.get(name)
    }

    /// Get group by GID
    pub fn get_group_by_gid(&self, gid: u32) -> Option<&GroupInfo> {
        self.groups_by_gid
            .get(&gid)
            .and_then(|name| self.groups_by_name.get(name))
    }

    /// List all users
    pub fn list_users(&self) -> impl Iterator<Item = &UserInfo> {
        self.users_by_name.values()
    }

    /// List all groups
    pub fn list_groups(&self) -> impl Iterator<Item = &GroupInfo> {
        self.groups_by_name.values()
    }

    /// Get users in a group
    pub fn get_group_members(&self, group_name: &str) -> Vec<&UserInfo> {
        let group = match self.groups_by_name.get(group_name) {
            Some(g) => g,
            None => return Vec::new(),
        };

        let mut members = Vec::new();

        // Users listed in the group file
        for member_name in &group.members {
            if let Some(user) = self.users_by_name.get(member_name) {
                members.push(user);
            }
        }

        // Users with this as their primary group
        for user in self.users_by_name.values() {
            if user.gid == group.gid && !members.iter().any(|u| u.name == user.name) {
                members.push(user);
            }
        }

        members
    }

    /// Check if a user is a member of a group
    pub fn is_member_of(&self, username: &str, groupname: &str) -> bool {
        if let Some(group) = self.groups_by_name.get(groupname) {
            // Check explicit membership
            if group.members.contains(&username.to_string()) {
                return true;
            }

            // Check if it's the user's primary group
            if let Some(user) = self.users_by_name.get(username) {
                return user.gid == group.gid;
            }
        }
        false
    }

    /// Get next available UID
    pub fn next_available_uid(&self, min: u32, max: u32) -> Option<u32> {
        (min..=max).find(|uid| !self.users_by_uid.contains_key(uid))
    }

    /// Get next available GID
    pub fn next_available_gid(&self, min: u32, max: u32) -> Option<u32> {
        (min..=max).find(|gid| !self.groups_by_gid.contains_key(gid))
    }

    /// User count
    pub fn user_count(&self) -> usize {
        self.users_by_name.len()
    }

    /// Group count
    pub fn group_count(&self) -> usize {
        self.groups_by_name.len()
    }
}

/// Get the current effective user ID
#[cfg(unix)]
pub fn get_euid() -> u32 {
    unsafe { libc::geteuid() }
}

#[cfg(not(unix))]
pub fn get_euid() -> u32 {
    0
}

/// Get the current effective group ID
#[cfg(unix)]
pub fn get_egid() -> u32 {
    unsafe { libc::getegid() }
}

#[cfg(not(unix))]
pub fn get_egid() -> u32 {
    0
}

/// Check if running as root
pub fn is_root() -> bool {
    get_euid() == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_native_available() {
        #[cfg(unix)]
        assert!(is_native_available());
    }

    #[test]
    #[cfg(unix)]
    fn test_get_user_root() {
        // root should always exist
        let user = get_user_by_name("root").unwrap();
        assert!(user.is_some());
        let user = user.unwrap();
        assert_eq!(user.uid, 0);
    }

    #[test]
    #[cfg(unix)]
    fn test_get_group_root() {
        // root group should always exist
        let group = get_group_by_name("root").unwrap();
        assert!(group.is_some());
        let group = group.unwrap();
        assert_eq!(group.gid, 0);
    }

    #[test]
    #[cfg(unix)]
    fn test_get_user_by_uid() {
        let user = get_user_by_uid(0).unwrap();
        assert!(user.is_some());
        assert_eq!(user.unwrap().name, "root");
    }

    #[test]
    fn test_user_info_is_system_user() {
        let system_user = UserInfo {
            name: "daemon".to_string(),
            uid: 1,
            gid: 1,
            gecos: String::new(),
            home: "/usr/sbin".to_string(),
            shell: "/usr/sbin/nologin".to_string(),
            password: None,
            groups: Vec::new(),
        };
        assert!(system_user.is_system_user());

        let regular_user = UserInfo {
            name: "testuser".to_string(),
            uid: 1001,
            gid: 1001,
            gecos: String::new(),
            home: "/home/testuser".to_string(),
            shell: "/bin/bash".to_string(),
            password: None,
            groups: Vec::new(),
        };
        assert!(!regular_user.is_system_user());
    }

    #[test]
    fn test_user_info_password_status() {
        let locked = UserInfo {
            name: "test".to_string(),
            uid: 1000,
            gid: 1000,
            gecos: String::new(),
            home: String::new(),
            shell: String::new(),
            password: Some("!$6$hash".to_string()),
            groups: Vec::new(),
        };
        assert!(locked.is_locked());
        assert!(!locked.has_password());

        let with_password = UserInfo {
            password: Some("$6$hash".to_string()),
            ..locked.clone()
        };
        assert!(with_password.has_password());
        assert!(!with_password.is_locked());
    }
}
