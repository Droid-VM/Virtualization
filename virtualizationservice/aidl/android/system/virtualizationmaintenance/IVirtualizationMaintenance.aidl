/*
 * Copyright 2024 The Android Open Source Project
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
package android.system.virtualizationmaintenance;

interface IVirtualizationMaintenance {
    /**
     * Notification that a package has been permanently removed, to allow related global state to
     * be removed.
     *
     * @param packageName Name of the package being removed.
     */
    void packageRemoved(in String packageName);

    /**
     * Notification that a user has been removed, to allow related global state to be removed.
     *
     * @param userId The Android user ID of the user (i.e. not the UID).
     */
    void userRemoved(int userId);
}
