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

#include <fcntl.h>
#include <string.h>
#include <time.h>
#include <unistd.h>

#include <algorithm>
#include <iomanip>
#include <iostream>
#include <random>

constexpr int kBlockSizeBytes = 4096;
constexpr int kNumBytesPerMB = 1024 * 1024;

int main(int argc, const char *argv[]) {
    if (argc != 5 || !(strcmp(argv[3], "rand") == 0 || strcmp(argv[3], "seq") == 0) ||
        !(strcmp(argv[4], "r") == 0 || strcmp(argv[4], "w") == 0)) {
        std::cerr << "Usage: " << argv[0] << " <filename> <file_size_mb> <rand|seq> <r|w>"
                  << std::endl;
        return EXIT_FAILURE;
    }
    int file_size_mb = std::stoi(argv[2]);
    bool is_rand = (strcmp(argv[3], "rand") == 0);
    bool is_read = (strcmp(argv[4], "r") == 0);
    const int block_count = file_size_mb * kNumBytesPerMB / kBlockSizeBytes;
    std::vector<int> offsets;
    if (is_rand) {
        std::mt19937 rd{std::random_device{}()};
        offsets.reserve(block_count);
        for (auto i = 0; i < block_count; ++i) offsets.push_back(i * kBlockSizeBytes);
        std::shuffle(offsets.begin(), offsets.end(), rd);
    }
    int fd(open(argv[1], is_read ? O_RDONLY : O_WRONLY | O_CREAT, 0644));
    if (fd == -1) {
        std::cerr << "failed to open file: " << argv[1] << std::endl;
        return EXIT_FAILURE;
    }

    char buf[kBlockSizeBytes];
    clock_t start = clock();
    for (auto i = 0; i < block_count; ++i) {
        if (is_rand) {
            if (lseek(fd, offsets[i], SEEK_SET) == -1) {
                std::cerr << "failed to lseek" << std::endl;
                return EXIT_FAILURE;
            }
        }
        auto bytes = is_read ? read(fd, buf, kBlockSizeBytes) : write(fd, buf, kBlockSizeBytes);
        if (bytes == 0) {
            std::cerr << "unexpected end of file" << std::endl;
            return EXIT_FAILURE;
        } else if (bytes == -1) {
            std::cerr << "failed to read" << std::endl;
            return EXIT_FAILURE;
        }
    }
    close(fd);
    double elapsed_seconds = ((double)clock() - start) / CLOCKS_PER_SEC;
    double rate = (double)file_size_mb / elapsed_seconds;
    std::cout << std::setprecision(12) << rate << std::endl;

    return EXIT_SUCCESS;
}
