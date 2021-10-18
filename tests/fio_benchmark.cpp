/*
 * Copyright (C) 2021 The Android Open Source Project
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

#include <benchmark/benchmark.h>
#include <errno.h>
#include <fcntl.h>
#include <stdio.h>
#include <stdlib.h>
#include <sys/stat.h>
#include <sys/time.h>
#include <sys/types.h>
#include <time.h>
#include <unistd.h>

#include <algorithm>
#include <iostream>
#include <random>
#include <string>
#include <vector>

namespace {

std::string testpath;

void SequentialReadBenchmark(benchmark::State& state) {
    for (auto _ : state) {
        int fd = open(testpath.c_str(), O_RDONLY);
        ssize_t total_read = 0;

        struct timeval st;
        gettimeofday(&st, nullptr);
        char buf[4096];
        for (ssize_t read_bytes; (read_bytes = TEMP_FAILURE_RETRY(read(fd, buf, 4096))) > 0;
             total_read += read_bytes)
            ;
        struct timeval en;
        gettimeofday(&en, nullptr);
        auto diffTime = (en.tv_sec - st.tv_sec) + ((en.tv_usec - st.tv_usec) / 1000000.0);

        printf("total %zd bytes, took %.3g seconds ", total_read, diffTime);

        double speed = total_read / diffTime;
        const char* unit;
        if (speed >= 1000) {
            speed /= 1024;
            unit = "KB";
        }
        if (speed >= 1000) {
            speed /= 1024;
            unit = "MB";
        }
        if (speed >= 1000) {
            speed /= 1024;
            unit = "GB";
        }
        printf("(%.3g %s/s)\n", speed, unit);
    }
}

void RandomReadBenchmark(benchmark::State& state) {
    std::mt19937 rd{std::random_device{}()};
    std::vector<off_t> vt;

    int fd = open(testpath.c_str(), O_RDONLY);

    auto fsize = ({
        struct stat st;
        fstat(fd, &st);
        (st.st_size + 4095) / 4096 * 4096;
    });

    vt.reserve(fsize / 4096);
    for (off_t i = 0; i < fsize / 4096; i++) vt.push_back(i * 4096);
    std::shuffle(vt.begin(), vt.end(), rd);
    for (auto _ : state) {
        ssize_t total_read = 0;

        struct timeval st;
        gettimeofday(&st, nullptr);
        char buf[4096];

        for (off_t off : vt) {
            lseek(fd, off, SEEK_SET);
            total_read += TEMP_FAILURE_RETRY(read(fd, buf, 4096));
        }
        struct timeval en;
        gettimeofday(&en, nullptr);
        auto diffTime = (en.tv_sec - st.tv_sec) + ((en.tv_usec - st.tv_usec) / 1000000.0);

        printf("total %zd bytes, took %.3g seconds ", total_read, diffTime);

        double speed = total_read / diffTime;
        const char* unit;
        if (speed >= 1000) {
            speed /= 1024;
            unit = "KB";
        }
        if (speed >= 1000) {
            speed /= 1024;
            unit = "MB";
        }
        if (speed >= 1000) {
            speed /= 1024;
            unit = "GB";
        }
        printf("(%.3g %s/s)\n", speed, unit);
    }
}

} // namespace

// Register the function as a benchmark
BENCHMARK(RandomReadBenchmark);
BENCHMARK(SequentialReadBenchmark);

int main(int argc, char** argv) {
    printf("file to test: ");
    std::getline(std::cin, testpath);

    ::benchmark::Initialize(&argc, argv);
    if (::benchmark::ReportUnrecognizedArguments(argc, argv)) return 1;
    ::benchmark::RunSpecifiedBenchmarks();
}
