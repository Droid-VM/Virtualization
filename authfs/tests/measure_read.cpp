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
#include <android-base/logging.h>
#include <android-base/parseint.h>
#include <android-base/unique_fd.h>
#include <time.h>

#include <random>

using android::base::ParseUint;
using android::base::unique_fd;

constexpr unsigned int kBlockSizeBytes = 4096;
constexpr unsigned int kNumBytesPerMB = 1024 * 1024;

int main(int argc, const char *argv[]) {
    unsigned int file_size_mb;
    if (argc != 4 || ParseUint(argv[2], &file_size_mb) ||
        !(strcmp(argv[3], "rand") || strcmp(argv[3], "seq"))) {
        LOG(ERROR) << "Usage: " << argv[0] << " <filename> <file_size_mb> <rand|seq>";
        return EXIT_FAILURE;
    }
    bool is_rand = strcmp(argv[3], "rand");
    const unsigned int block_count = file_size_mb * kNumBytesPerMB / kBlockSizeBytes;
    std::vector<unsigned int> offsets;
    if (is_rand) {
        std::mt19937 rd{std::random_device{}()};
        offsets.reserve(block_count);
        for (auto i = 0; i < block_count; ++i) offsets.push_back(i * kBlockSizeBytes);
        std::shuffle(offsets.begin(), offsets.end(), rd);
    }
    unique_fd fd(open(argv[1], O_RDONLY | O_CLOEXEC));
    if (fd.get() == -1) {
        LOG(ERROR) << "Read: opening " << argv[1] << " failed";
        return EXIT_FAILURE;
    }

    char buf[kBlockSizeBytes];
    clock_t start = clock();
    for (auto i = 0; i < block_count; ++i) {
        if (is_rand) {
            if (lseek(fd.get(), offsets[i], SEEK_SET) == -1) {
                LOG(ERROR) << "failed to lseek";
                return EXIT_FAILURE;
            }
        }
        auto bytes = read(fd.get(), buf, kBlockSizeBytes);
        if (bytes == 0) {
            LOG(ERROR) << "unexpected end of file";
            return EXIT_FAILURE;
        } else if (bytes == -1) {
            LOG(ERROR) << "failed to read";
            return EXIT_FAILURE;
        }
    }
    double elapsed_seconds = ((double)clock() - start) / CLOCKS_PER_SEC;
    double read_rate = (double)file_size_mb / elapsed_seconds;
    printf("%lf", read_rate);

    return EXIT_SUCCESS;
}
