#pragma once

#include <windows.h>

#include <algorithm>
#include <array>
#include <cstddef>
#include <cstdint>
#include <cstring>
#include <limits>
#include <optional>
#include <string_view>
#include <utility>
#include <vector>

namespace shared_iat {

enum class Error {
  invalid_pe,
  invalid_import_directory,
  shared_import_absent,
  shared_import_duplicated,
  unexpected_shared_import,
  incomplete_shared_imports,
  invalid_iat,
  protection_failed,
};

class Tripwire final {
 public:
  Tripwire() = default;
  ~Tripwire() { (void)restore(); }

  Tripwire(const Tripwire &) = delete;
  Tripwire &operator=(const Tripwire &) = delete;

  Tripwire(Tripwire &&other) noexcept
      : iat_begin_(std::exchange(other.iat_begin_, nullptr)),
        byte_length_(std::exchange(other.byte_length_, 0)),
        original_(std::move(other.original_)),
        armed_(std::exchange(other.armed_, false)) {}

  Tripwire &operator=(Tripwire &&other) noexcept {
    if (this != &other) {
      (void)restore();
      iat_begin_ = std::exchange(other.iat_begin_, nullptr);
      byte_length_ = std::exchange(other.byte_length_, 0);
      original_ = std::move(other.original_);
      armed_ = std::exchange(other.armed_, false);
    }
    return *this;
  }

  [[nodiscard]] std::size_t import_count() const noexcept {
    return original_.size();
  }

  [[nodiscard]] std::uintptr_t first_patched_target_for_test() const noexcept {
    if (!armed_ || iat_begin_ == nullptr || original_.empty()) {
      return 0;
    }
    return static_cast<std::uintptr_t>(iat_begin_[0].u1.Function);
  }

  [[nodiscard]] bool restore() noexcept {
    if (!armed_) {
      return true;
    }
    DWORD previous_protection = 0;
    if (!VirtualProtect(iat_begin_, byte_length_, PAGE_READWRITE,
                        &previous_protection)) {
      return false;
    }
    for (std::size_t index = 0; index < original_.size(); ++index) {
      iat_begin_[index].u1.Function = original_[index];
    }
    MemoryBarrier();
    (void)FlushInstructionCache(GetCurrentProcess(), iat_begin_, byte_length_);
    DWORD ignored = 0;
    const bool protected_again =
        VirtualProtect(iat_begin_, byte_length_, previous_protection, &ignored) !=
        FALSE;
    if (protected_again) {
      armed_ = false;
    }
    return protected_again;
  }

  [[nodiscard]] static std::optional<Tripwire> arm(
      HMODULE module, std::uintptr_t replacement, Error &error) {
    constexpr std::array<std::string_view, 13> kExpectedImports = {
        "??0uCallStackTracker@@QEAA@PEBD@Z",
        "??1uCallStackTracker@@QEAA@XZ",
        "GetInfiniteWaitEvent",
        "stricmp_utf8",
        "stricmp_utf8_ex",
        "uAppendMenu",
        "uBugCheck",
        "uExceptFilterProc",
        "uFormatSystemErrorMessage",
        "uGetOpenFileName",
        "uPrintCrashInfo_OnEvent",
        "uPrintfV",
        "uSetDlgItemText",
    };

    if (module == nullptr || replacement == 0) {
      error = Error::invalid_pe;
      return std::nullopt;
    }
    auto *base = reinterpret_cast<std::byte *>(module);
    const auto *dos = reinterpret_cast<const IMAGE_DOS_HEADER *>(base);
    if (dos->e_magic != IMAGE_DOS_SIGNATURE || dos->e_lfanew <= 0) {
      error = Error::invalid_pe;
      return std::nullopt;
    }
    const auto *nt = reinterpret_cast<const IMAGE_NT_HEADERS64 *>(
        base + static_cast<std::size_t>(dos->e_lfanew));
    if (nt->Signature != IMAGE_NT_SIGNATURE ||
        nt->FileHeader.Machine != IMAGE_FILE_MACHINE_AMD64 ||
        nt->OptionalHeader.Magic != IMAGE_NT_OPTIONAL_HDR64_MAGIC) {
      error = Error::invalid_pe;
      return std::nullopt;
    }
    const std::uint32_t image_size = nt->OptionalHeader.SizeOfImage;
    const IMAGE_DATA_DIRECTORY directory =
        nt->OptionalHeader.DataDirectory[IMAGE_DIRECTORY_ENTRY_IMPORT];
    if (directory.VirtualAddress == 0 ||
        directory.Size < sizeof(IMAGE_IMPORT_DESCRIPTOR) ||
        !rva_fits(directory.VirtualAddress, directory.Size, image_size)) {
      error = Error::invalid_import_directory;
      return std::nullopt;
    }

    auto *descriptors = reinterpret_cast<IMAGE_IMPORT_DESCRIPTOR *>(
        base + directory.VirtualAddress);
    const std::size_t descriptor_limit =
        directory.Size / sizeof(IMAGE_IMPORT_DESCRIPTOR);
    IMAGE_IMPORT_DESCRIPTOR *shared_descriptor = nullptr;
    for (std::size_t index = 0; index < descriptor_limit; ++index) {
      IMAGE_IMPORT_DESCRIPTOR &descriptor = descriptors[index];
      if (descriptor.Name == 0 && descriptor.FirstThunk == 0) {
        break;
      }
      const std::optional<std::string_view> name =
          image_string(base, image_size, descriptor.Name);
      if (!name.has_value()) {
        error = Error::invalid_import_directory;
        return std::nullopt;
      }
      if (ascii_iequals(*name, "shared.dll")) {
        if (shared_descriptor != nullptr) {
          error = Error::shared_import_duplicated;
          return std::nullopt;
        }
        shared_descriptor = &descriptor;
      }
    }
    if (shared_descriptor == nullptr) {
      error = Error::shared_import_absent;
      return std::nullopt;
    }
    if (shared_descriptor->OriginalFirstThunk == 0 ||
        shared_descriptor->FirstThunk == 0) {
      error = Error::invalid_iat;
      return std::nullopt;
    }

    const std::uint32_t names_rva = shared_descriptor->OriginalFirstThunk;
    const std::uint32_t iat_rva = shared_descriptor->FirstThunk;
    if (!rva_fits(names_rva, sizeof(IMAGE_THUNK_DATA64), image_size) ||
        !rva_fits(iat_rva, sizeof(IMAGE_THUNK_DATA64), image_size)) {
      error = Error::invalid_iat;
      return std::nullopt;
    }
    const auto *names =
        reinterpret_cast<const IMAGE_THUNK_DATA64 *>(base + names_rva);
    auto *iat = reinterpret_cast<IMAGE_THUNK_DATA64 *>(base + iat_rva);
    const std::size_t name_limit =
        (image_size - names_rva) / sizeof(IMAGE_THUNK_DATA64);
    const std::size_t iat_limit =
        (image_size - iat_rva) / sizeof(IMAGE_THUNK_DATA64);
    const std::size_t thunk_limit = std::min(name_limit, iat_limit);
    std::array<bool, kExpectedImports.size()> seen{};
    std::size_t import_count = 0;
    bool terminated = false;
    for (; import_count < thunk_limit; ++import_count) {
      const std::uint64_t name_value = names[import_count].u1.AddressOfData;
      if (name_value == 0) {
        terminated = true;
        break;
      }
      if (IMAGE_SNAP_BY_ORDINAL64(name_value) ||
          name_value > std::numeric_limits<std::uint32_t>::max()) {
        error = Error::unexpected_shared_import;
        return std::nullopt;
      }
      const std::uint32_t import_rva = static_cast<std::uint32_t>(name_value);
      if (!rva_fits(import_rva, offsetof(IMAGE_IMPORT_BY_NAME, Name) + 1,
                    image_size)) {
        error = Error::invalid_iat;
        return std::nullopt;
      }
      const std::uint32_t text_rva =
          import_rva + offsetof(IMAGE_IMPORT_BY_NAME, Name);
      const std::optional<std::string_view> import_name =
          image_string(base, image_size, text_rva);
      if (!import_name.has_value()) {
        error = Error::invalid_iat;
        return std::nullopt;
      }
      const auto expected =
          std::find(kExpectedImports.begin(), kExpectedImports.end(),
                    *import_name);
      if (expected == kExpectedImports.end()) {
        error = Error::unexpected_shared_import;
        return std::nullopt;
      }
      const std::size_t expected_index =
          static_cast<std::size_t>(expected - kExpectedImports.begin());
      if (seen[expected_index]) {
        error = Error::unexpected_shared_import;
        return std::nullopt;
      }
      seen[expected_index] = true;
    }
    if (!terminated || import_count != kExpectedImports.size() ||
        !std::all_of(seen.begin(), seen.end(), [](bool value) { return value; })) {
      error = Error::incomplete_shared_imports;
      return std::nullopt;
    }

    Tripwire result;
    result.iat_begin_ = iat;
    result.byte_length_ = import_count * sizeof(IMAGE_THUNK_DATA64);
    result.original_.reserve(import_count);
    for (std::size_t index = 0; index < import_count; ++index) {
      result.original_.push_back(iat[index].u1.Function);
    }

    DWORD previous_protection = 0;
    if (!VirtualProtect(result.iat_begin_, result.byte_length_, PAGE_READWRITE,
                        &previous_protection)) {
      error = Error::protection_failed;
      return std::nullopt;
    }
    const std::uint64_t replacement_value =
        static_cast<std::uint64_t>(replacement);
    for (std::size_t index = 0; index < import_count; ++index) {
      result.iat_begin_[index].u1.Function = replacement_value;
    }
    MemoryBarrier();
    (void)FlushInstructionCache(GetCurrentProcess(), result.iat_begin_,
                                result.byte_length_);
    DWORD ignored = 0;
    if (!VirtualProtect(result.iat_begin_, result.byte_length_,
                        previous_protection, &ignored)) {
      for (std::size_t index = 0; index < import_count; ++index) {
        result.iat_begin_[index].u1.Function = result.original_[index];
      }
      DWORD second_ignored = 0;
      (void)VirtualProtect(result.iat_begin_, result.byte_length_,
                           previous_protection, &second_ignored);
      error = Error::protection_failed;
      return std::nullopt;
    }
    result.armed_ = true;
    return result;
  }

 private:
  [[nodiscard]] static bool rva_fits(std::uint32_t rva, std::size_t length,
                                     std::uint32_t image_size) noexcept {
    return rva <= image_size && length <= image_size - rva;
  }

  [[nodiscard]] static std::optional<std::string_view> image_string(
      const std::byte *base, std::uint32_t image_size,
      std::uint32_t rva) noexcept {
    if (rva >= image_size) {
      return std::nullopt;
    }
    const char *start = reinterpret_cast<const char *>(base + rva);
    const std::size_t maximum = image_size - rva;
    const void *terminator = std::memchr(start, '\0', maximum);
    if (terminator == nullptr) {
      return std::nullopt;
    }
    const auto *end = static_cast<const char *>(terminator);
    return std::string_view(start, static_cast<std::size_t>(end - start));
  }

  [[nodiscard]] static bool ascii_iequals(std::string_view left,
                                          std::string_view right) noexcept {
    if (left.size() != right.size()) {
      return false;
    }
    for (std::size_t index = 0; index < left.size(); ++index) {
      const auto lower = [](char character) {
        return character >= 'A' && character <= 'Z'
                   ? static_cast<char>(character - 'A' + 'a')
                   : character;
      };
      if (lower(left[index]) != lower(right[index])) {
        return false;
      }
    }
    return true;
  }

  IMAGE_THUNK_DATA64 *iat_begin_ = nullptr;
  std::size_t byte_length_ = 0;
  std::vector<std::uint64_t> original_;
  bool armed_ = false;
};

}  // namespace shared_iat
