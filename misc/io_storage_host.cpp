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

#include <android-base/file.h>
#include <android-base/result.h>
#include <time.h>

using android::base::ErrnoError;
using android::base::Error;
using android::base::Result;
using android::base::unique_fd;
using android::base::WriteStringToFd;

constexpr int kBlockSizeBytes = 4096;
constexpr int kNumBytesPerMB = 1024 * 1024;

/**
 * Measures the read rate for reading the given file.
 * @return The read rate in MB/s.
 */
Result<double> measure_read_rate(const std::string& filename, int64_t expected_size,
                                 bool measure_latency) {
    struct stat file_stats;
    if (stat(filename.c_str(), &file_stats) == -1) {
        return Error() << "failed to get file stats";
    }
    int64_t file_size_bytes = file_stats.st_size;
    if (file_size_bytes < expected_size) {
        // printf("Strangely stat file size is unexpected %ld, expected %ld\n", file_size_bytes,
        // expected_size );
        file_size_bytes = expected_size;
    }

    const int64_t block_count = file_size_bytes / kBlockSizeBytes;
    std::vector<uint64_t> offsets(block_count);
    for (auto i = 0; i < block_count; ++i) {
        offsets[i] = i * kBlockSizeBytes;
    }
    char buf[kBlockSizeBytes];
    unique_fd fd(open(filename.c_str(), O_RDONLY | O_CLOEXEC));
    struct timespec start;
    if (clock_gettime(CLOCK_MONOTONIC, &start) == -1) {
        return ErrnoError() << "failed to clock_gettime";
    }

    if (fd.get() == -1) {
        return ErrnoError() << "Read: opening " << filename << " failed";
    }
    for (auto i = 0; i < block_count; ++i) {
        auto bytes = pread(fd, buf, kBlockSizeBytes, offsets[i]);
        if (bytes == 0) {
            return Error() << "unexpected end of file";
        } else if (bytes == -1) {
            return ErrnoError() << "failed to read";
        }
        if (i == 0 && measure_latency) {
            struct timespec finish;
            if (clock_gettime(CLOCK_MONOTONIC, &finish) == -1) {
                return ErrnoError() << "failed to clock_gettime";
            }
            double latency_measure = (finish.tv_sec - start.tv_sec) * 1000.0 +
                    (finish.tv_nsec - start.tv_nsec) / 1e6;
            printf("Read latency_measure in millisecond %f\n", latency_measure);
            return {0};
        }
    }
    struct timespec finish;
    if (clock_gettime(CLOCK_MONOTONIC, &finish) == -1) {
        return ErrnoError() << "failed to clock_gettime";
    }
    double elapsed_seconds = finish.tv_sec - start.tv_sec + (finish.tv_nsec - start.tv_nsec) / 1e9;
    double file_size_mb = (double)file_size_bytes / kNumBytesPerMB;
    return {file_size_mb / elapsed_seconds};
}

/**
 * Measures the throughput of writing random data to the given file.
 * @return The write rate in MB/s.
 */
Result<double> measure_write_rate(const std::string& filename, int64_t size_bytes,
                                  bool measure_latency) {
    struct stat file_stats;
    const int64_t block_count = size_bytes / kBlockSizeBytes;
    char buf[kBlockSizeBytes];
    int fd_rand = open("/dev/urandom", O_RDONLY);
    read(fd_rand, buf, kBlockSizeBytes);
    // TODO(b/390648694): Ideally open with O_SYNC instead of syncfs().
    unique_fd fd(open(filename.c_str(), O_CREAT | O_WRONLY, 00666));

    struct timespec start;
    if (clock_gettime(CLOCK_MONOTONIC, &start) == -1) {
        return ErrnoError() << "failed to clock_gettime";
    }
    if (fd.get() == -1) {
        return ErrnoError() << "Write: opening " << filename << " failed";
    }
    if (stat(filename.c_str(), &file_stats) == -1) {
        return Error() << "failed to get file stats";
    }

    for (auto i = 0; i < block_count; ++i) {
        auto bytes = write(fd, buf, kBlockSizeBytes);
        if (bytes == 0) {
            return Error() << "unexpected end of file";
        } else if (bytes == -1) {
            return ErrnoError() << "failed to write";
        }

        if (measure_latency && i == 0) {
            syncfs(fd);
            struct timespec finish;
            if (clock_gettime(CLOCK_MONOTONIC, &finish) == -1) {
                return ErrnoError() << "failed to clock_gettime";
            }
            double latency_measure = (finish.tv_sec - start.tv_sec) * 1000.0 +
                    (finish.tv_nsec - start.tv_nsec) / 1e6;
            printf("Write latency_measure in millisecond %f\n", latency_measure);
            return {0};
        }
    }
    syncfs(fd);
    struct timespec finish;
    if (clock_gettime(CLOCK_MONOTONIC, &finish) == -1) {
        return ErrnoError() << "failed to clock_gettime";
    }
    double elapsed_seconds = finish.tv_sec - start.tv_sec + (finish.tv_nsec - start.tv_nsec) / 1e9;
    double file_size_mb = (double)size_bytes / kNumBytesPerMB;
    return {file_size_mb / elapsed_seconds};
}

int main(int argc, char* argv[]) {
    int n = 5;
    printf("Testing...");
    int64_t size = (1073741824 / 4) * 3;
    double sum_r = 0, sum_w = 0;
    if (argc < 3) {
        printf("Require 3+ args, supplied %d", argc);
        return 0;
    }

    std::vector<uint64_t> read_rates, write_rates;
    for (auto i = 0; i < n; ++i) {
        Result<double> w_rate, r_rate;
        if (strcmp(argv[1], "write") == 0) {
            w_rate =
                    measure_write_rate(argv[2], size, argc == 4 && strcmp(argv[3], "latency") == 0);
        }
        if (strcmp(argv[1], "read") == 0) {
            r_rate = measure_read_rate(argv[2], size, argc == 4 && strcmp(argv[3], "latency") == 0);
        }
        write_rates.push_back(w_rate.value());
        read_rates.push_back(r_rate.value());
        printf("Write rate {%f}\n", w_rate.value());
        printf("Read rate {%f}\n", r_rate.value());
        sum_w += w_rate.value();
        sum_r += r_rate.value();
    }
    printf("Average Write rate {%f} MB/s\n", sum_w / n);
    printf("Average Read rate {%f} MB/s\n", sum_r / n);
    return 0;
}
