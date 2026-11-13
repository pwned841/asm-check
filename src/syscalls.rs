//! The Linux x86-64 system call table, for the calls hand-written assembly
//! actually uses.

use crate::abi;
use crate::reg::RegId;

/// A system call. Parameter names starting with `*` are pointers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Syscall {
    pub number: i64,
    pub name: &'static str,
    params: &'static [&'static str],
}

impl Syscall {
    pub fn arity(&self) -> usize {
        self.params.len()
    }

    /// Name of the parameter at `index`, without the pointer marker.
    pub fn param(&self, index: usize) -> &'static str {
        self.params[index].trim_start_matches('*')
    }

    pub fn is_pointer(&self, index: usize) -> bool {
        self.params[index].starts_with('*')
    }

    /// `write(fd, buf, count)`.
    pub fn signature(&self) -> String {
        let params: Vec<&str> = (0..self.arity()).map(|index| self.param(index)).collect();
        format!("{}({})", self.name, params.join(", "))
    }

    /// The registers holding the arguments, in order.
    pub fn registers(&self) -> &'static [RegId] {
        &abi::SYSCALL_ARGUMENTS[..self.arity()]
    }
}

type Row = (i64, &'static str, &'static [&'static str]);

#[rustfmt::skip]
const TABLE: &[Row] = &[
    (0, "read", &["fd", "*buf", "count"]),
    (1, "write", &["fd", "*buf", "count"]),
    (2, "open", &["*path", "flags", "mode"]),
    (3, "close", &["fd"]),
    (4, "stat", &["*path", "*statbuf"]),
    (5, "fstat", &["fd", "*statbuf"]),
    (6, "lstat", &["*path", "*statbuf"]),
    (7, "poll", &["*fds", "nfds", "timeout"]),
    (8, "lseek", &["fd", "offset", "whence"]),
    (9, "mmap", &["addr", "length", "prot", "flags", "fd", "offset"]),
    (10, "mprotect", &["addr", "length", "prot"]),
    (11, "munmap", &["addr", "length"]),
    (12, "brk", &["addr"]),
    (13, "rt_sigaction", &["signum", "*act", "*oldact", "sigsetsize"]),
    (14, "rt_sigprocmask", &["how", "*set", "*oldset", "sigsetsize"]),
    (15, "rt_sigreturn", &[]),
    (16, "ioctl", &["fd", "request", "arg"]),
    (17, "pread64", &["fd", "*buf", "count", "offset"]),
    (18, "pwrite64", &["fd", "*buf", "count", "offset"]),
    (19, "readv", &["fd", "*iov", "iovcnt"]),
    (20, "writev", &["fd", "*iov", "iovcnt"]),
    (21, "access", &["*path", "mode"]),
    (22, "pipe", &["*pipefd"]),
    (23, "select", &["nfds", "*readfds", "*writefds", "*exceptfds", "*timeout"]),
    (24, "sched_yield", &[]),
    (32, "dup", &["fd"]),
    (33, "dup2", &["oldfd", "newfd"]),
    (34, "pause", &[]),
    (35, "nanosleep", &["*req", "*rem"]),
    (37, "alarm", &["seconds"]),
    (39, "getpid", &[]),
    (40, "sendfile", &["out_fd", "in_fd", "*offset", "count"]),
    (41, "socket", &["domain", "type", "protocol"]),
    (42, "connect", &["fd", "*addr", "addrlen"]),
    (43, "accept", &["fd", "*addr", "*addrlen"]),
    (44, "sendto", &["fd", "*buf", "len", "flags", "*dest_addr", "addrlen"]),
    (45, "recvfrom", &["fd", "*buf", "len", "flags", "*src_addr", "*addrlen"]),
    (48, "shutdown", &["fd", "how"]),
    (49, "bind", &["fd", "*addr", "addrlen"]),
    (50, "listen", &["fd", "backlog"]),
    (54, "setsockopt", &["fd", "level", "optname", "*optval", "optlen"]),
    (56, "clone", &["flags", "stack", "*parent_tid", "*child_tid", "tls"]),
    (57, "fork", &[]),
    (58, "vfork", &[]),
    (59, "execve", &["*path", "*argv", "*envp"]),
    (60, "exit", &["status"]),
    (61, "wait4", &["pid", "*wstatus", "options", "*rusage"]),
    (62, "kill", &["pid", "sig"]),
    (63, "uname", &["*buf"]),
    (72, "fcntl", &["fd", "cmd", "arg"]),
    (74, "fsync", &["fd"]),
    (76, "truncate", &["*path", "length"]),
    (77, "ftruncate", &["fd", "length"]),
    (78, "getdents", &["fd", "*dirp", "count"]),
    (79, "getcwd", &["*buf", "size"]),
    (80, "chdir", &["*path"]),
    (82, "rename", &["*oldpath", "*newpath"]),
    (83, "mkdir", &["*path", "mode"]),
    (84, "rmdir", &["*path"]),
    (85, "creat", &["*path", "mode"]),
    (86, "link", &["*oldpath", "*newpath"]),
    (87, "unlink", &["*path"]),
    (88, "symlink", &["*target", "*linkpath"]),
    (89, "readlink", &["*path", "*buf", "bufsiz"]),
    (90, "chmod", &["*path", "mode"]),
    (92, "chown", &["*path", "owner", "group"]),
    (95, "umask", &["mask"]),
    (96, "gettimeofday", &["*tv", "*tz"]),
    (99, "sysinfo", &["*info"]),
    (101, "ptrace", &["request", "pid", "addr", "data"]),
    (102, "getuid", &[]),
    (104, "getgid", &[]),
    (105, "setuid", &["uid"]),
    (106, "setgid", &["gid"]),
    (107, "geteuid", &[]),
    (108, "getegid", &[]),
    (110, "getppid", &[]),
    (186, "gettid", &[]),
    (201, "time", &["*tloc"]),
    (217, "getdents64", &["fd", "*dirp", "count"]),
    (228, "clock_gettime", &["clockid", "*tp"]),
    (230, "clock_nanosleep", &["clockid", "flags", "*req", "*rem"]),
    (231, "exit_group", &["status"]),
    (257, "openat", &["dirfd", "*path", "flags", "mode"]),
    (262, "newfstatat", &["dirfd", "*path", "*statbuf", "flags"]),
    (263, "unlinkat", &["dirfd", "*path", "flags"]),
    (288, "accept4", &["fd", "*addr", "*addrlen", "flags"]),
    (293, "pipe2", &["*pipefd", "flags"]),
    (318, "getrandom", &["*buf", "buflen", "flags"]),
    (319, "memfd_create", &["*name", "flags"]),
    (322, "execveat", &["dirfd", "*path", "*argv", "*envp", "flags"]),
];

/// The system call with this number, if it is in the table.
pub fn lookup(number: i64) -> Option<Syscall> {
    TABLE
        .binary_search_by_key(&number, |row| row.0)
        .ok()
        .map(|index| TABLE[index])
        .map(|(number, name, params)| Syscall { number, name, params })
}

/// Whether `number` cannot be a system call: negative, in the range x86-64
/// never assigned (335-423), or far beyond the end of the table.
pub fn is_impossible(number: i64) -> bool {
    !(0..=600).contains(&number) || (335..=423).contains(&number)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_is_sorted_and_fits_the_registers() {
        assert!(TABLE.windows(2).all(|pair| pair[0].0 < pair[1].0));
        assert!(TABLE.iter().all(|row| row.2.len() <= abi::SYSCALL_ARGUMENTS.len()));
    }

    #[test]
    fn looks_up() {
        let write = lookup(1).unwrap();
        assert_eq!(write.signature(), "write(fd, buf, count)");
        assert!(write.is_pointer(1) && !write.is_pointer(0));
        assert_eq!(write.registers(), [RegId::RDI, RegId::RSI, RegId::RDX]);
        assert_eq!(lookup(60).unwrap().name, "exit");
        assert_eq!(lookup(25), None);
        assert!(is_impossible(-1) && is_impossible(400) && !is_impossible(1));
    }
}
