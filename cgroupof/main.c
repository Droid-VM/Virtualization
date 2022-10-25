/*
 * Copyright (C) 2022 The Android Open Source Project
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

#include <errno.h>
#include <linux/limits.h>
#include <stdbool.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

// Return cgroup of the given pid by reading /proc/<pid>/cgroup. This is
// implemented as a separate binary instead of as a library because reading the
// file requires some unusual privileges and we often don't want to give the
// privileges to the callers.
//
// 1. AID_READPROC group. The /proc is mounted with `gid=3009 hidepid=2` which
// means only the process in the group 3009 (AID_READPROC) can see /proc/<pid>
// entries other than /proc/self.
//
// 2. read access to domain:file. /proc/<pid>/cgroup files are labeled as the
// domain type of the owning process. In order for a process to read "any"
// /proc/<pid>/cgroup, the process needs to have read access to domain:file,
// which is too permissive for most processes.
//
// With this implemented as a binary, we don't need to grant above privileges to
// the callers. The privileges are granted only to this binary, and the callers
// are only required to have access to execute this binary.

int main(int argc, const char* argv[]) {
    if (argc != 2) {
        fprintf(stderr, "Usage: %s <pid>\n", argv[0]);
        return 1;
    }

    const char* pid = argv[1];
    if (atoi(pid) == 0) {
        fprintf(stderr, "%s is not a number.\n", pid);
        return 1;
    }

    char cgroup_path[PATH_MAX];
    if (snprintf(cgroup_path, PATH_MAX, "/proc/%s/cgroup", pid) < 0) {
        fprintf(stderr, "%s is too long.\n", pid);
        return 1;
    }

    FILE* f = fopen(cgroup_path, "r");
    if (f == NULL) {
        fprintf(stderr, "Cannot open %s: %s.\n", cgroup_path, strerror(errno));
        return 1;
    }

    bool found = false;
    char* line = NULL;
    ssize_t read;
    size_t len = 0;
    while ((read = getline(&line, &len, f)) != -1) {
        // Find a line starts with 0::/
        // See https://docs.kernel.org/admin-guide/cgroup-v2.html
        char* cgroup = strstr(line, "0::/");
        if (cgroup != line) continue;
        found = true;

        fputs(cgroup + 4, stdout); // print the string after the marker
        break;
    }

    if (line != NULL) {
        free(line);
    }
    fclose(f);

    if (!found) {
        fprintf(stderr, "Cannot find cgroup of PID %s,\n", pid);
        return 1;
    }

    return 0;
}
