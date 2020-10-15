/*
 * Copyright (C) 2020 The Android Open Source Project
 *
 * Licensed under the Apache License, Version 2.0 (the "License");
 * you may not use this file except in compliance with the License.
 * You may obtain a copy of the License at
 *
 *      http://www.apache.org/licenses/LICENSE-2.0
 *
 * Unless required by applicable law or agreed to in writing, software
 * distributed under the License is distributed on an "AS IS" BASIS,
 * WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
 * See the License for the specific language governing permissions and
 * limitations under the License.
 */

#include <dirent.h>
#include <sys/mount.h>
#include <sys/stat.h>
#include <sys/types.h>
#include <sys/utsname.h>
#include <sys/wait.h>
#include <unistd.h>

#include <cstdlib>
#include <filesystem>
#include <fstream>
#include <iostream>
#include <memory>
#include <string>
#include <vector>

#include <modprobe/modprobe.h>

static constexpr const char LOG_TAG[] = "guest: ";

#define MODULE_BASE_DIR "/lib/modules"

std::string GetModuleLoadList(bool recovery, const std::string& dir_path) {
    auto module_load_file = "modules.load";
    if (recovery) {
        struct stat fileStat;
        std::string recovery_load_path = dir_path + "/modules.load.recovery";
        if (!stat(recovery_load_path.c_str(), &fileStat)) {
            module_load_file = "modules.load.recovery";
        }
    }

    return module_load_file;
}

bool LoadKernelModules(bool recovery, bool want_console) {
    struct utsname uts;
    if (uname(&uts)) {
        std::cerr << "Failed to get kernel version." << std::endl;
        return false;
    }
    int major, minor;
    if (sscanf(uts.release, "%d.%d", &major, &minor) != 2) {
        std::cerr << "Failed to parse kernel version " << uts.release << std::endl;
        return false;
    }

    std::unique_ptr<DIR, decltype(&closedir)> base_dir(opendir(MODULE_BASE_DIR), closedir);
    if (!base_dir) {
        std::cerr << "Unable to open /lib/modules, skipping module loading." << std::endl;
        return false;
    }
    dirent* entry;
    std::vector<std::string> module_dirs;
    while ((entry = readdir(base_dir.get()))) {
        if (entry->d_type != DT_DIR) {
            continue;
        }
        int dir_major, dir_minor;
        if (sscanf(entry->d_name, "%d.%d", &dir_major, &dir_minor) != 2 || dir_major != major ||
            dir_minor != minor) {
            continue;
        }
        module_dirs.emplace_back(entry->d_name);
    }

    // Sort the directories so they are iterated over during module loading
    // in a consistent order. Alphabetical sorting is fine here because the
    // kernel version at the beginning of the directory name must match the
    // current kernel version, so the sort only applies to a label that
    // follows the kernel version, for example /lib/modules/5.4 vs.
    // /lib/modules/5.4-gki.
    std::sort(module_dirs.begin(), module_dirs.end());

    for (const auto& module_dir : module_dirs) {
        std::string dir_path = MODULE_BASE_DIR "/";
        dir_path.append(module_dir);
        Modprobe m({dir_path}, GetModuleLoadList(recovery, dir_path));
        bool retval = m.LoadListedModules(!want_console);
        int modules_loaded = m.GetModuleCount();
        if (modules_loaded > 0) {
            return retval;
        }
    }

    Modprobe m({MODULE_BASE_DIR}, GetModuleLoadList(recovery, MODULE_BASE_DIR));
    bool retval = m.LoadListedModules(!want_console);
    int modules_loaded = m.GetModuleCount();
    if (modules_loaded > 0) {
        return retval;
    }

    return true;
}

int main(int argc, const char *argv[]) {
    std::cerr << LOG_TAG << "Guest VM init process" << std::endl;

    std::cerr << LOG_TAG << "Command line args: ";
    for (int i = 0; i < argc + 1; ++i) {
        std::cerr << (argv[i] ? argv[i] : "<null>") << " ";
    }
    std::cerr << std::endl;

    if (clearenv() != EXIT_SUCCESS) {
        std::cerr << LOG_TAG << "clearenv() failed" << std::endl;
        return EXIT_FAILURE;
    }

    std::cerr << LOG_TAG << "Loading kernel modules..." << std::endl;
    if (!LoadKernelModules(false, false)) {
        std::cerr << LOG_TAG << "LoadKernelModules failed" << std::endl;
        return EXIT_FAILURE;
    }

    std::cerr << LOG_TAG << "Executing test binary " << argv[1] << "..." << std::endl;
    execv(argv[1], (char**)(argv+1));
    std::cerr << LOG_TAG << "execv() failed: " << strerror(errno) << std::endl;
    return EXIT_FAILURE;
}
