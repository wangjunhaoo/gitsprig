"""读取 macOS 公共进程资源接口，分别报告 RSS 与系统 footprint。"""
import ctypes
import json
import os
import sys


class RUsageInfoV2(ctypes.Structure):
    _fields_ = [("ri_uuid", ctypes.c_uint8 * 16)] + [
        (name, ctypes.c_uint64) for name in (
            "ri_user_time", "ri_system_time", "ri_pkg_idle_wkups",
            "ri_interrupt_wkups", "ri_pageins", "ri_wired_size",
            "ri_resident_size", "ri_phys_footprint", "ri_proc_start_abstime",
            "ri_proc_exit_abstime", "ri_child_user_time", "ri_child_system_time",
            "ri_child_pkg_idle_wkups", "ri_child_interrupt_wkups",
            "ri_child_pageins", "ri_child_elapsed_abstime",
            "ri_diskio_bytesread", "ri_diskio_byteswritten",
        )
    ]


if ctypes.sizeof(RUsageInfoV2) != 160 or RUsageInfoV2.ri_phys_footprint.offset != 72:
    raise RuntimeError("当前平台的资源结构布局不匹配。")

library = ctypes.CDLL("/usr/lib/libproc.dylib", use_errno=True)
library.proc_pid_rusage.argtypes = [ctypes.c_int, ctypes.c_int, ctypes.c_void_p]
library.proc_pid_rusage.restype = ctypes.c_int


def process_memory(pid):
    usage = RUsageInfoV2()
    if library.proc_pid_rusage(pid, 2, ctypes.byref(usage)) != 0:
        error = ctypes.get_errno()
        raise OSError(error, os.strerror(error))
    return {"residentBytes": usage.ri_resident_size, "footprintBytes": usage.ri_phys_footprint}


if __name__ == "__main__":
    pids = [int(value) for value in sys.argv[1:]] if len(sys.argv) > 1 else [os.getpid()]
    print(json.dumps({pid: process_memory(pid) for pid in pids}, indent=2))
