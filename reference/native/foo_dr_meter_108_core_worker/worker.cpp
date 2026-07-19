#include "mini_json.hpp"
#include "shared_iat_tripwire.hpp"

#include <windows.h>
#include <bcrypt.h>

#include <algorithm>
#include <array>
#include <bit>
#include <charconv>
#include <cmath>
#include <cstddef>
#include <cstdint>
#include <cstdio>
#include <cstring>
#include <initializer_list>
#include <limits>
#include <memory>
#include <stdexcept>
#include <string>
#include <string_view>
#include <utility>
#include <vector>

extern "C" void mm108_store_x87_control_word(std::uint16_t *value) noexcept;
extern "C" void mm108_load_x87_control_word(std::uint16_t value) noexcept;
extern "C" void mm108_store_mxcsr(std::uint32_t *value) noexcept;
extern "C" void mm108_load_mxcsr(std::uint32_t value) noexcept;

namespace {

constexpr std::string_view kTargetSha256 =
    "ff3556add231859c2f3ddfa111312720c8d4969270416229a7bd26f73ba22489";
constexpr std::uint64_t kTargetByteLength = 424448;
constexpr std::uint32_t kInitRva = 0x8410;
constexpr std::uint32_t kPushRva = 0x89F0;
constexpr std::uint32_t kFinishRva = 0x8DF0;
constexpr std::uint32_t kResultCleanupRva = 0x3CE0;
constexpr std::uint32_t kHistogramCleanupRva = 0x10970;
constexpr std::uint32_t kChannelStateCleanupRva = 0x109E0;
constexpr std::uint32_t kAccumulatorCleanupRva = 0x10A70;
constexpr std::size_t kSessionSize = 0x70;
constexpr std::size_t kResultSize = 0x58;
constexpr std::size_t kChannelStateStride = 0x28;
constexpr std::uint32_t kMaximumChannels = 64;
constexpr std::uint32_t kMaximumBlockFrames = 1'048'576;
constexpr std::uint64_t kMaximumPcmBytes = UINT64_C(1) << 30;
constexpr std::uint64_t kMaximumArtifactBytes = UINT64_C(64) << 20;
constexpr std::uint64_t kMaximumRequestBytes = UINT64_C(1) << 20;
constexpr std::uint16_t kAppliedX87ControlWord = 0x037F;
constexpr std::uint32_t kAppliedMxcsr = 0x00001F80;
constexpr std::uint32_t kSharedShimMarker = 0x4D4D5348;

static_assert(sizeof(float) == 4);
static_assert(sizeof(double) == 8);
static_assert(sizeof(void *) == 8);
static_assert(std::numeric_limits<float>::is_iec559);
static_assert(std::numeric_limits<double>::is_iec559);
static_assert(std::endian::native == std::endian::little);

extern "C" [[noreturn]] void __fastcall
macinmeter_shared_core_tripwire() noexcept {
  for (;;) {
    RaiseFailFastException(nullptr, nullptr, 0);
    TerminateProcess(GetCurrentProcess(), 0xC0000409U);
  }
}

struct KnownRuntimeIdentity {
  std::string_view name;
  std::string_view sha256;
  std::uint64_t byte_length;
};

constexpr std::array<KnownRuntimeIdentity, 4> kRealRuntime = {{
    {"shared.dll",
     "f860ee48f9e88a4da575c8114a82a11e3d25ceb9c8ce3405f646917cf07c7e4d",
     142336},
    {"msvcp140.dll",
     "003da4807acdc912e67edba49be574daa5238bb7acff871d8666d16f8072ff89",
     579920},
    {"vcruntime140.dll",
     "a8f950b4357ec12cfccddc9094cca56a3d5244b95e09ea6e9a746489f2d58736",
     109392},
    {"vcruntime140_1.dll",
     "e4b533a94e02c574780e4b333fcf0889f65ed00d39e32c0fbbda2116f185873f",
     49520},
}};

constexpr std::array<std::string_view, 4> kRuntimeNames = {
    "shared.dll", "msvcp140.dll", "vcruntime140.dll", "vcruntime140_1.dll"};

class WorkerError final : public std::runtime_error {
 public:
  WorkerError(std::string code, std::string message)
      : std::runtime_error(std::move(message)), code_(std::move(code)) {}

  [[nodiscard]] const std::string &code() const noexcept { return code_; }

 private:
  std::string code_;
};

[[noreturn]] void fail(std::string_view code, std::string_view message) {
  throw WorkerError(std::string(code), std::string(message));
}

class UniqueHandle final {
 public:
  UniqueHandle() noexcept = default;
  explicit UniqueHandle(HANDLE handle) noexcept : handle_(handle) {}
  ~UniqueHandle() { reset(); }

  UniqueHandle(const UniqueHandle &) = delete;
  UniqueHandle &operator=(const UniqueHandle &) = delete;

  UniqueHandle(UniqueHandle &&other) noexcept
      : handle_(std::exchange(other.handle_, INVALID_HANDLE_VALUE)) {}

  UniqueHandle &operator=(UniqueHandle &&other) noexcept {
    if (this != &other) {
      reset(std::exchange(other.handle_, INVALID_HANDLE_VALUE));
    }
    return *this;
  }

  [[nodiscard]] HANDLE get() const noexcept { return handle_; }

  [[nodiscard]] bool valid() const noexcept {
    return handle_ != nullptr && handle_ != INVALID_HANDLE_VALUE;
  }

  void reset(HANDLE next = INVALID_HANDLE_VALUE) noexcept {
    if (valid()) {
      CloseHandle(handle_);
    }
    handle_ = next;
  }

 private:
  HANDLE handle_ = INVALID_HANDLE_VALUE;
};

class UniqueModule final {
 public:
  UniqueModule() noexcept = default;
  explicit UniqueModule(HMODULE module) noexcept : module_(module) {}
  ~UniqueModule() { reset(); }

  UniqueModule(const UniqueModule &) = delete;
  UniqueModule &operator=(const UniqueModule &) = delete;

  [[nodiscard]] HMODULE get() const noexcept { return module_; }

  [[nodiscard]] bool unload() noexcept {
    if (module_ == nullptr) {
      return true;
    }
    HMODULE current = std::exchange(module_, nullptr);
    return FreeLibrary(current) != FALSE;
  }

  void reset(HMODULE next = nullptr) noexcept {
    if (module_ != nullptr) {
      FreeLibrary(module_);
    }
    module_ = next;
  }

 private:
  HMODULE module_ = nullptr;
};

struct FileIdentity {
  std::string sha256;
  std::uint64_t byte_length = 0;
};

struct ObjectIdentity {
  std::uint64_t volume_serial_number = 0;
  std::array<std::byte, 16> file_id{};

  [[nodiscard]] bool operator==(const ObjectIdentity &) const noexcept =
      default;
};

struct LockedFile {
  UniqueHandle handle;
  std::vector<std::byte> bytes;
  FileIdentity identity;
  ObjectIdentity object_identity;
};

struct RuntimeArtifact {
  std::string name;
  std::wstring source_path;
  FileIdentity identity;
};

struct Request {
  std::string request_id;
  std::wstring target_path;
  FileIdentity target_identity;
  std::string runtime_profile;
  std::vector<RuntimeArtifact> runtime_artifacts;
  std::uint32_t sample_rate = 0;
  std::uint32_t channels = 0;
  std::uint64_t frames = 0;
  std::wstring pcm_path;
  FileIdentity pcm_identity;
  std::uint32_t block_frames = 0;
};

struct ResponseContext {
  std::string request_id = "unknown";
  std::string target_sha256 = std::string(kTargetSha256);
};

struct SessionSnapshot {
  std::uint32_t current_window_frames = 0;
  std::uint64_t window_count = 0;
  std::uint64_t submitted_frames = 0;
};

struct ChannelState {
  std::uint64_t rms_square_sum_bits = 0;
  std::uint64_t primary_peak_bits = 0;
  std::uint64_t secondary_peak_bits = 0;
  std::uint64_t primary_peak_key_bits = 0;
  std::uint64_t secondary_peak_key_bits = 0;
};

struct ChannelResult {
  std::uint32_t dr_bits = 0;
  std::uint32_t peak_bits = 0;
  std::uint32_t rms_bits = 0;
};

struct FpControlPair {
  std::uint16_t x87_control_word = 0;
  std::uint32_t mxcsr = 0;
};

struct FpEnvironmentRecord {
  FpControlPair before;
  FpControlPair applied;
  FpControlPair after;
  FpControlPair restored;
};

struct CoreOutput {
  std::uint32_t track_dr_bits = 0;
  SessionSnapshot before_finish;
  SessionSnapshot after_finish;
  std::vector<ChannelState> channel_state;
  std::vector<ChannelResult> channel_results;
  FpEnvironmentRecord fp_environment;
};

template <typename T>
[[nodiscard]] T load_unaligned(const void *source) noexcept {
  T value{};
  std::memcpy(&value, source, sizeof(value));
  return value;
}

[[nodiscard]] bool is_lower_hex(std::string_view value,
                                std::size_t length) noexcept {
  if (value.size() != length) {
    return false;
  }
  return std::all_of(value.begin(), value.end(), [](char character) {
    return (character >= '0' && character <= '9') ||
           (character >= 'a' && character <= 'f');
  });
}

[[nodiscard]] std::string json_escape(std::string_view value) {
  std::string output;
  output.reserve(value.size() + 2);
  output.push_back('"');
  constexpr char kHex[] = "0123456789abcdef";
  for (const unsigned char character : value) {
    switch (character) {
      case '"':
        output += "\\\"";
        break;
      case '\\':
        output += "\\\\";
        break;
      case '\b':
        output += "\\b";
        break;
      case '\f':
        output += "\\f";
        break;
      case '\n':
        output += "\\n";
        break;
      case '\r':
        output += "\\r";
        break;
      case '\t':
        output += "\\t";
        break;
      default:
        if (character < 0x20) {
          output += "\\u00";
          output.push_back(kHex[character >> 4]);
          output.push_back(kHex[character & 0x0F]);
        } else {
          output.push_back(static_cast<char>(character));
        }
        break;
    }
  }
  output.push_back('"');
  return output;
}

template <typename T>
[[nodiscard]] std::string decimal(T value) {
  std::array<char, 32> buffer{};
  const auto result =
      std::to_chars(buffer.data(), buffer.data() + buffer.size(), value);
  if (result.ec != std::errc{}) {
    fail("internal_error", "numeric rendering failed");
  }
  return std::string(buffer.data(), result.ptr);
}

template <typename T>
[[nodiscard]] std::string fixed_hex(T value) {
  constexpr std::size_t kDigits = sizeof(T) * 2;
  constexpr char kHex[] = "0123456789abcdef";
  std::array<char, kDigits> output{};
  for (std::size_t index = 0; index < kDigits; ++index) {
    const std::size_t shift = (kDigits - index - 1) * 4;
    output[index] = kHex[(value >> shift) & static_cast<T>(0x0F)];
  }
  return std::string(output.data(), output.size());
}

void write_all(HANDLE handle, std::string_view bytes) {
  std::size_t offset = 0;
  while (offset < bytes.size()) {
    const auto remaining = bytes.size() - offset;
    const DWORD chunk = static_cast<DWORD>(
        std::min<std::size_t>(remaining, std::numeric_limits<DWORD>::max()));
    DWORD written = 0;
    if (!WriteFile(handle, bytes.data() + offset, chunk, &written, nullptr) ||
        written == 0) {
      fail("output_failed", "worker protocol output failed");
    }
    offset += written;
  }
}

[[nodiscard]] bool environment_is_one(const wchar_t *name) noexcept {
  std::array<wchar_t, 2> enabled{};
  return GetEnvironmentVariableW(name, enabled.data(),
                                 static_cast<DWORD>(enabled.size())) == 1 &&
         enabled[0] == L'1';
}

void trace_checkpoint(std::string_view checkpoint) noexcept {
  if (!environment_is_one(L"MACINMETER_CORE_TRACE")) {
    return;
  }
  const std::string line = "core-worker: " + std::string(checkpoint) + "\n";
  DWORD written = 0;
  (void)WriteFile(GetStdHandle(STD_ERROR_HANDLE), line.data(),
                  static_cast<DWORD>(line.size()), &written, nullptr);
}

[[nodiscard]] std::wstring utf8_to_wide(std::string_view value) {
  if (value.empty() ||
      value.size() > static_cast<std::size_t>(std::numeric_limits<int>::max())) {
    fail("invalid_request", "request path is invalid");
  }
  const int required =
      MultiByteToWideChar(CP_UTF8, MB_ERR_INVALID_CHARS, value.data(),
                          static_cast<int>(value.size()), nullptr, 0);
  if (required <= 0) {
    fail("invalid_request", "request path is not valid UTF-8");
  }
  std::wstring output(static_cast<std::size_t>(required), L'\0');
  if (MultiByteToWideChar(CP_UTF8, MB_ERR_INVALID_CHARS, value.data(),
                          static_cast<int>(value.size()), output.data(),
                          required) != required) {
    fail("invalid_request", "request path conversion failed");
  }
  return output;
}

[[nodiscard]] std::wstring full_path(const std::wstring &path) {
  const DWORD required = GetFullPathNameW(path.c_str(), 0, nullptr, nullptr);
  if (required == 0 || required > 32768) {
    fail("invalid_request", "request path cannot be resolved");
  }
  std::wstring output(required, L'\0');
  const DWORD written =
      GetFullPathNameW(path.c_str(), required, output.data(), nullptr);
  if (written == 0 || written >= required) {
    fail("invalid_request", "request path cannot be resolved");
  }
  output.resize(written);
  return output;
}

[[nodiscard]] std::wstring basename(const std::wstring &path) {
  const std::size_t separator = path.find_last_of(L"\\/");
  return separator == std::wstring::npos ? path : path.substr(separator + 1);
}

[[nodiscard]] bool same_path(const std::wstring &left,
                             const std::wstring &right) {
  const std::wstring left_full = full_path(left);
  const std::wstring right_full = full_path(right);
  return _wcsicmp(left_full.c_str(), right_full.c_str()) == 0;
}

[[nodiscard]] std::string sha256(const std::vector<std::byte> &bytes) {
  BCRYPT_ALG_HANDLE algorithm = nullptr;
  BCRYPT_HASH_HANDLE hash = nullptr;
  std::vector<UCHAR> object;
  std::array<UCHAR, 32> digest{};

  auto cleanup = [&]() noexcept {
    if (hash != nullptr) {
      BCryptDestroyHash(hash);
    }
    if (algorithm != nullptr) {
      BCryptCloseAlgorithmProvider(algorithm, 0);
    }
  };

  if (BCryptOpenAlgorithmProvider(&algorithm, BCRYPT_SHA256_ALGORITHM, nullptr,
                                  0) != 0) {
    fail("hash_failed", "SHA-256 provider initialization failed");
  }
  DWORD object_length = 0;
  DWORD copied = 0;
  if (BCryptGetProperty(algorithm, BCRYPT_OBJECT_LENGTH,
                        reinterpret_cast<PUCHAR>(&object_length),
                        sizeof(object_length), &copied, 0) != 0 ||
      copied != sizeof(object_length)) {
    cleanup();
    fail("hash_failed", "SHA-256 object sizing failed");
  }
  object.resize(object_length);
  if (BCryptCreateHash(algorithm, &hash, object.data(), object_length, nullptr,
                       0, 0) != 0) {
    cleanup();
    fail("hash_failed", "SHA-256 initialization failed");
  }
  std::size_t offset = 0;
  while (offset < bytes.size()) {
    const ULONG chunk = static_cast<ULONG>(std::min<std::size_t>(
        bytes.size() - offset, std::numeric_limits<ULONG>::max()));
    if (BCryptHashData(
            hash,
            reinterpret_cast<PUCHAR>(
                const_cast<std::byte *>(bytes.data() + offset)),
            chunk, 0) != 0) {
      cleanup();
      fail("hash_failed", "SHA-256 update failed");
    }
    offset += chunk;
  }
  if (BCryptFinishHash(hash, digest.data(),
                       static_cast<ULONG>(digest.size()), 0) != 0) {
    cleanup();
    fail("hash_failed", "SHA-256 finalization failed");
  }
  cleanup();

  constexpr char kHex[] = "0123456789abcdef";
  std::string output(digest.size() * 2, '\0');
  for (std::size_t index = 0; index < digest.size(); ++index) {
    output[index * 2] = kHex[digest[index] >> 4];
    output[index * 2 + 1] = kHex[digest[index] & 0x0F];
  }
  return output;
}

[[nodiscard]] ObjectIdentity query_object_identity(
    HANDLE handle, std::string_view error_code) {
  FILE_ID_INFO information{};
  if (!GetFileInformationByHandleEx(handle, FileIdInfo, &information,
                                    static_cast<DWORD>(sizeof(information)))) {
    fail(error_code, "filesystem object identity is unavailable");
  }
  ObjectIdentity identity;
  identity.volume_serial_number =
      static_cast<std::uint64_t>(information.VolumeSerialNumber);
  std::memcpy(identity.file_id.data(), information.FileId.Identifier,
              identity.file_id.size());
  return identity;
}

[[nodiscard]] LockedFile read_locked_file(const std::wstring &path,
                                          std::uint64_t maximum_bytes,
                                          std::string_view error_code) {
  UniqueHandle handle(CreateFileW(
      path.c_str(), GENERIC_READ, FILE_SHARE_READ, nullptr, OPEN_EXISTING,
      FILE_ATTRIBUTE_NORMAL | FILE_FLAG_SEQUENTIAL_SCAN |
          FILE_FLAG_OPEN_REPARSE_POINT,
      nullptr));
  if (!handle.valid() || GetFileType(handle.get()) != FILE_TYPE_DISK) {
    fail(error_code, "required input file cannot be opened");
  }
  BY_HANDLE_FILE_INFORMATION file_information{};
  if (!GetFileInformationByHandle(handle.get(), &file_information) ||
      (file_information.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT) != 0) {
    fail(error_code, "required input file is not a direct regular file");
  }
  LARGE_INTEGER size{};
  if (!GetFileSizeEx(handle.get(), &size) || size.QuadPart < 0 ||
      static_cast<std::uint64_t>(size.QuadPart) > maximum_bytes ||
      static_cast<std::uint64_t>(size.QuadPart) >
          std::numeric_limits<std::size_t>::max()) {
    fail(error_code, "required input file has an invalid size");
  }
  std::vector<std::byte> bytes(static_cast<std::size_t>(size.QuadPart));
  std::size_t offset = 0;
  while (offset < bytes.size()) {
    const DWORD chunk = static_cast<DWORD>(std::min<std::size_t>(
        bytes.size() - offset, std::numeric_limits<DWORD>::max()));
    DWORD read = 0;
    if (!ReadFile(handle.get(), bytes.data() + offset, chunk, &read, nullptr) ||
        read == 0) {
      fail(error_code, "required input file could not be read completely");
    }
    offset += read;
  }
  std::byte extra{};
  DWORD extra_read = 0;
  if (!ReadFile(handle.get(), &extra, 1, &extra_read, nullptr) ||
      extra_read != 0) {
    fail(error_code, "required input file changed while being read");
  }
  FileIdentity identity{sha256(bytes), static_cast<std::uint64_t>(bytes.size())};
  ObjectIdentity object_identity =
      query_object_identity(handle.get(), error_code);
  return LockedFile{std::move(handle), std::move(bytes), std::move(identity),
                    object_identity};
}

void verify_identity(const FileIdentity &actual, const FileIdentity &expected,
                     std::string_view code, std::string_view message) {
  if (actual.sha256 != expected.sha256 ||
      actual.byte_length != expected.byte_length) {
    fail(code, message);
  }
}

void require_exact_keys(
    const mini_json::Value &value,
    std::initializer_list<std::string_view> expected_keys) {
  const auto &object = value.as_object();
  if (object.size() != expected_keys.size()) {
    fail("invalid_request", "request object fields differ from the protocol");
  }
  for (const std::string_view key : expected_keys) {
    if (value.find(key) == nullptr) {
      fail("invalid_request", "request object fields differ from the protocol");
    }
  }
}

[[nodiscard]] std::uint64_t request_uint(const mini_json::Value &value,
                                         std::string_view key) {
  try {
    return value.at(key).as_uint();
  } catch (const mini_json::TypeError &) {
    fail("invalid_request", "request field has the wrong JSON type");
  }
}

[[nodiscard]] const std::string &request_string(
    const mini_json::Value &value, std::string_view key) {
  try {
    return value.at(key).as_string();
  } catch (const mini_json::TypeError &) {
    fail("invalid_request", "request field has the wrong JSON type");
  }
}

[[nodiscard]] bool request_bool(const mini_json::Value &value,
                                std::string_view key) {
  try {
    return value.at(key).as_bool();
  } catch (const mini_json::TypeError &) {
    fail("invalid_request", "request field has the wrong JSON type");
  }
}

[[nodiscard]] const mini_json::Value &request_value(
    const mini_json::Value &value, std::string_view key) {
  try {
    return value.at(key);
  } catch (const mini_json::TypeError &) {
    fail("invalid_request", "request field is missing");
  }
}

[[nodiscard]] std::uint32_t narrow_u32(std::uint64_t value,
                                       std::string_view message) {
  if (value > std::numeric_limits<std::uint32_t>::max()) {
    fail("invalid_request", message);
  }
  return static_cast<std::uint32_t>(value);
}

void validate_identifier(std::string_view value) {
  if (value.empty() || value.size() > 128) {
    fail("invalid_request", "request identifier is invalid");
  }
  const auto first = value.front();
  if (!((first >= 'A' && first <= 'Z') || (first >= 'a' && first <= 'z') ||
        (first >= '0' && first <= '9'))) {
    fail("invalid_request", "request identifier is invalid");
  }
  for (const char character : value.substr(1)) {
    if (!((character >= 'A' && character <= 'Z') ||
          (character >= 'a' && character <= 'z') ||
          (character >= '0' && character <= '9') || character == '.' ||
          character == '_' || character == ':' || character == '-')) {
      fail("invalid_request", "request identifier is invalid");
    }
  }
}

[[nodiscard]] Request parse_request(std::string_view raw,
                                    ResponseContext &context) {
  mini_json::Value root;
  try {
    root = mini_json::parse(raw);
  } catch (const mini_json::ParseError &) {
    fail("invalid_request", "request is not strict JSON");
  }
  try {
    require_exact_keys(root, {"schemaVersion", "kind", "requestId", "target",
                              "stream", "pcm", "options"});
    if (request_uint(root, "schemaVersion") != 1 ||
        request_string(root, "kind") != "foo_dr_meter_108_core_request") {
      fail("invalid_request", "request protocol identity is unsupported");
    }

    Request request;
    request.request_id = request_string(root, "requestId");
    validate_identifier(request.request_id);
    if (!is_lower_hex(request.request_id, 64)) {
      fail("invalid_request", "request ID is not a lowercase SHA-256 value");
    }
    context.request_id = request.request_id;

    const auto &target = request_value(root, "target");
    require_exact_keys(target,
                       {"dllPath", "sha256", "byteLength", "initRva",
                        "pushRva", "finishRva", "runtimeProfile",
                        "runtimeArtifacts"});
    request.target_path = full_path(utf8_to_wide(request_string(target, "dllPath")));
    request.target_identity.sha256 = request_string(target, "sha256");
    request.target_identity.byte_length = request_uint(target, "byteLength");
    context.target_sha256 = request.target_identity.sha256;
    if (request.target_identity.sha256 != kTargetSha256 ||
        request.target_identity.byte_length != kTargetByteLength ||
        request_uint(target, "initRva") != kInitRva ||
        request_uint(target, "pushRva") != kPushRva ||
        request_uint(target, "finishRva") != kFinishRva) {
      fail("target_identity_mismatch", "fixed target identity is incorrect");
    }
    request.runtime_profile = request_string(target, "runtimeProfile");
    if (request.runtime_profile != "fixed_foobar_2_25_10" &&
        request.runtime_profile != "fail_fast_shared_v1") {
      fail("invalid_request", "runtime profile is unsupported");
    }

    const auto &artifact_values =
        request_value(target, "runtimeArtifacts").as_array();
    if (artifact_values.size() != kRuntimeNames.size()) {
      fail("invalid_request", "runtime artifact allowlist is invalid");
    }
    for (std::size_t index = 0; index < artifact_values.size(); ++index) {
      const auto &item = artifact_values[index];
      require_exact_keys(item, {"name", "sourcePath", "sha256", "byteLength"});
      RuntimeArtifact artifact;
      artifact.name = request_string(item, "name");
      if (artifact.name != kRuntimeNames[index]) {
        fail("invalid_request", "runtime artifacts are not in canonical order");
      }
      artifact.source_path =
          full_path(utf8_to_wide(request_string(item, "sourcePath")));
      if (_wcsicmp(basename(artifact.source_path).c_str(),
                   utf8_to_wide(artifact.name).c_str()) != 0) {
        fail("invalid_request", "runtime artifact basename is invalid");
      }
      artifact.identity.sha256 = request_string(item, "sha256");
      artifact.identity.byte_length = request_uint(item, "byteLength");
      if (!is_lower_hex(artifact.identity.sha256, 64) ||
          artifact.identity.byte_length == 0 ||
          artifact.identity.byte_length > kMaximumArtifactBytes) {
        fail("invalid_request", "runtime artifact identity is invalid");
      }
      const KnownRuntimeIdentity &known = kRealRuntime[index];
      if (index != 0 || request.runtime_profile == "fixed_foobar_2_25_10") {
        if (artifact.identity.sha256 != known.sha256 ||
            artifact.identity.byte_length != known.byte_length) {
          fail("runtime_identity_mismatch",
               "runtime artifact differs from the fixed profile");
        }
      } else if (artifact.identity.sha256 == kRealRuntime[0].sha256) {
        fail("runtime_identity_mismatch",
             "fail-fast profile cannot use the real shared runtime");
      }
      request.runtime_artifacts.push_back(std::move(artifact));
    }

    const auto &stream = request_value(root, "stream");
    require_exact_keys(
        stream, {"sampleRate", "channels", "frames", "sampleEncoding"});
    request.sample_rate =
        narrow_u32(request_uint(stream, "sampleRate"), "sample rate is invalid");
    request.channels =
        narrow_u32(request_uint(stream, "channels"), "channel count is invalid");
    request.frames = request_uint(stream, "frames");
    if (request.sample_rate == 0 || request.channels == 0 ||
        request.channels > kMaximumChannels ||
        request_string(stream, "sampleEncoding") !=
            "f64le-interleaved") {
      fail("invalid_request", "stream geometry or encoding is invalid");
    }
    if (request.frames >
        std::numeric_limits<std::uint64_t>::max() / request.channels / 8) {
      fail("invalid_request", "PCM byte length overflows");
    }
    const std::uint64_t expected_pcm_bytes =
        request.frames * request.channels * 8;
    if (expected_pcm_bytes > kMaximumPcmBytes) {
      fail("resource_limit", "PCM exceeds the worker memory limit");
    }

    const auto &pcm = request_value(root, "pcm");
    require_exact_keys(pcm, {"path", "sha256", "byteLength"});
    request.pcm_path = full_path(utf8_to_wide(request_string(pcm, "path")));
    request.pcm_identity.sha256 = request_string(pcm, "sha256");
    request.pcm_identity.byte_length = request_uint(pcm, "byteLength");
    if (!is_lower_hex(request.pcm_identity.sha256, 64) ||
        request.pcm_identity.byte_length != expected_pcm_bytes) {
      fail("invalid_request", "PCM identity or byte length is invalid");
    }

    const auto &options = request_value(root, "options");
    require_exact_keys(options,
                       {"multichannelLoudnessWeighting", "blockFrames"});
    if (request_bool(options, "multichannelLoudnessWeighting")) {
      fail("invalid_request", "multichannel weighting must be disabled");
    }
    request.block_frames =
        narrow_u32(request_uint(options, "blockFrames"), "block size is invalid");
    if (request.block_frames == 0 ||
        request.block_frames > kMaximumBlockFrames) {
      fail("invalid_request", "block size is invalid");
    }
    return request;
  } catch (const mini_json::TypeError &) {
    fail("invalid_request", "request field has the wrong JSON type");
  }
}

[[nodiscard]] std::vector<double> decode_pcm(
    const std::vector<std::byte> &bytes) {
  if (bytes.size() % sizeof(std::uint64_t) != 0) {
    fail("pcm_identity_mismatch", "PCM is not binary64 aligned");
  }
  std::vector<double> samples(bytes.size() / sizeof(std::uint64_t));
  for (std::size_t index = 0; index < samples.size(); ++index) {
    const std::uint64_t bits =
        load_unaligned<std::uint64_t>(bytes.data() + index * 8);
    samples[index] = std::bit_cast<double>(bits);
    if (!std::isfinite(samples[index])) {
      fail("invalid_pcm", "PCM contains a non-finite sample");
    }
  }
  return samples;
}

class PrivateDirectorySecurity final {
 public:
  PrivateDirectorySecurity() {
    HANDLE raw_token = nullptr;
    if (!OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &raw_token)) {
      fail("staging_failed",
           "private staging security could not inspect the process token");
    }
    UniqueHandle token(raw_token);

    DWORD token_bytes = 0;
    if (GetTokenInformation(token.get(), TokenUser, nullptr, 0, &token_bytes) ||
        GetLastError() != ERROR_INSUFFICIENT_BUFFER || token_bytes == 0) {
      fail("staging_failed",
           "private staging security could not size the process identity");
    }
    token_user_.resize(token_bytes);
    if (!GetTokenInformation(token.get(), TokenUser, token_user_.data(),
                             token_bytes, &token_bytes)) {
      fail("staging_failed",
           "private staging security could not read the process identity");
    }
    const auto *token_user =
        reinterpret_cast<const TOKEN_USER *>(token_user_.data());
    if (!IsValidSid(token_user->User.Sid)) {
      fail("staging_failed",
           "private staging security found an invalid process identity");
    }

    DWORD system_sid_bytes = static_cast<DWORD>(system_sid_.size());
    if (!CreateWellKnownSid(WinLocalSystemSid, nullptr, system_sid_.data(),
                            &system_sid_bytes)) {
      fail("staging_failed",
           "private staging security could not create the system identity");
    }
    PSID system_sid = system_sid_.data();
    const DWORD user_sid_bytes = GetLengthSid(token_user->User.Sid);
    const DWORD actual_system_sid_bytes = GetLengthSid(system_sid);
    if (user_sid_bytes == 0 || actual_system_sid_bytes == 0) {
      fail("staging_failed",
           "private staging security found an invalid identity length");
    }
    constexpr std::size_t kAceOverhead =
        sizeof(ACCESS_ALLOWED_ACE) - sizeof(DWORD);
    const std::size_t acl_bytes =
        sizeof(ACL) + 2 * kAceOverhead + user_sid_bytes +
        actual_system_sid_bytes;
    if (acl_bytes > std::numeric_limits<DWORD>::max()) {
      fail("staging_failed", "private staging ACL is too large");
    }
    acl_.resize(acl_bytes);
    auto *acl = reinterpret_cast<ACL *>(acl_.data());
    if (!InitializeAcl(acl, static_cast<DWORD>(acl_.size()), ACL_REVISION)) {
      fail("staging_failed", "private staging ACL initialization failed");
    }
    constexpr DWORD kInheritance =
        OBJECT_INHERIT_ACE | CONTAINER_INHERIT_ACE;
    if (!AddAccessAllowedAceEx(acl, ACL_REVISION, kInheritance,
                               FILE_ALL_ACCESS, token_user->User.Sid) ||
        !AddAccessAllowedAceEx(acl, ACL_REVISION, kInheritance,
                               FILE_ALL_ACCESS, system_sid)) {
      fail("staging_failed", "private staging ACL construction failed");
    }
    if (!InitializeSecurityDescriptor(&descriptor_,
                                      SECURITY_DESCRIPTOR_REVISION) ||
        !SetSecurityDescriptorDacl(&descriptor_, TRUE, acl, FALSE) ||
        !SetSecurityDescriptorControl(&descriptor_, SE_DACL_PROTECTED,
                                      SE_DACL_PROTECTED)) {
      fail("staging_failed",
           "private staging security descriptor construction failed");
    }
    attributes_.nLength = static_cast<DWORD>(sizeof(attributes_));
    attributes_.lpSecurityDescriptor = &descriptor_;
    attributes_.bInheritHandle = FALSE;
  }

  PrivateDirectorySecurity(const PrivateDirectorySecurity &) = delete;
  PrivateDirectorySecurity &operator=(const PrivateDirectorySecurity &) =
      delete;

  [[nodiscard]] SECURITY_ATTRIBUTES *attributes() noexcept {
    return &attributes_;
  }

 private:
  std::vector<std::byte> token_user_;
  std::array<std::byte, SECURITY_MAX_SID_SIZE> system_sid_{};
  std::vector<std::byte> acl_;
  SECURITY_DESCRIPTOR descriptor_{};
  SECURITY_ATTRIBUTES attributes_{};
};

class StageDirectory final {
 public:
  StageDirectory() {
    std::array<wchar_t, 32768> temporary_root{};
    const DWORD root_length = GetTempPathW(
        static_cast<DWORD>(temporary_root.size()), temporary_root.data());
    if (root_length == 0 || root_length >= temporary_root.size()) {
      fail("staging_failed", "private staging directory could not be created");
    }
    PrivateDirectorySecurity security;
    std::array<unsigned char, 16> random{};
    for (unsigned int attempt = 0; attempt < 32; ++attempt) {
      if (BCryptGenRandom(nullptr, random.data(),
                          static_cast<ULONG>(random.size()),
                          BCRYPT_USE_SYSTEM_PREFERRED_RNG) != 0) {
        fail("staging_failed", "private staging randomness failed");
      }
      std::wstring suffix = L"macinmeter-core-";
      constexpr wchar_t kHex[] = L"0123456789abcdef";
      for (const unsigned char byte : random) {
        suffix.push_back(kHex[byte >> 4]);
        suffix.push_back(kHex[byte & 0x0F]);
      }
      path_.assign(temporary_root.data(), root_length);
      path_ += suffix;
      if (CreateDirectoryW(path_.c_str(), security.attributes())) {
        lock_and_verify();
        return;
      }
      if (GetLastError() != ERROR_ALREADY_EXISTS) {
        break;
      }
    }
    fail("staging_failed", "private staging directory could not be created");
  }

  ~StageDirectory() {
    // The no-delete directory handle is required through target unload, but it
    // must be released before this worker can remove its own staging tree.
    directory_handle_.reset();
    for (const std::wstring &file : files_) {
      DeleteFileW(file.c_str());
    }
    if (!path_.empty()) {
      RemoveDirectoryW(path_.c_str());
    }
  }

  StageDirectory(const StageDirectory &) = delete;
  StageDirectory &operator=(const StageDirectory &) = delete;

  [[nodiscard]] std::wstring file_path(std::wstring_view name) const {
    return path_ + L"\\" + std::wstring(name);
  }

  void remember(std::wstring path) { files_.push_back(std::move(path)); }

  void verify_path_identity() const {
    UniqueHandle by_path = open_direct_directory(path_);
    if (query_object_identity(by_path.get(), "staging_failed") != identity_) {
      fail("staging_failed",
           "private staging path no longer names the locked directory");
    }
  }

 private:
  [[nodiscard]] static UniqueHandle open_direct_directory(
      const std::wstring &path) {
    UniqueHandle handle(CreateFileW(
        path.c_str(), FILE_READ_ATTRIBUTES | READ_CONTROL,
        FILE_SHARE_READ | FILE_SHARE_WRITE, nullptr, OPEN_EXISTING,
        FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT, nullptr));
    if (!handle.valid() || GetFileType(handle.get()) != FILE_TYPE_DISK) {
      fail("staging_failed",
           "private staging directory could not be locked");
    }
    BY_HANDLE_FILE_INFORMATION information{};
    if (!GetFileInformationByHandle(handle.get(), &information) ||
        (information.dwFileAttributes & FILE_ATTRIBUTE_DIRECTORY) == 0 ||
        (information.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT) != 0) {
      fail("staging_failed",
           "private staging path is not a direct directory");
    }
    return handle;
  }

  void lock_and_verify() {
    directory_handle_ = open_direct_directory(path_);
    identity_ =
        query_object_identity(directory_handle_.get(), "staging_failed");
    verify_path_identity();
  }

  std::wstring path_;
  std::vector<std::wstring> files_;
  UniqueHandle directory_handle_;
  ObjectIdentity identity_;
};

void write_staged_file(const std::wstring &path,
                       const std::vector<std::byte> &bytes) {
  UniqueHandle handle(CreateFileW(path.c_str(), GENERIC_WRITE, 0, nullptr,
                                  CREATE_NEW, FILE_ATTRIBUTE_NORMAL, nullptr));
  if (!handle.valid()) {
    fail("staging_failed", "private staged file could not be created");
  }
  std::size_t offset = 0;
  while (offset < bytes.size()) {
    const DWORD chunk = static_cast<DWORD>(std::min<std::size_t>(
        bytes.size() - offset, std::numeric_limits<DWORD>::max()));
    DWORD written = 0;
    if (!WriteFile(handle.get(), bytes.data() + offset, chunk, &written,
                   nullptr) ||
        written == 0) {
      fail("staging_failed", "private staged file could not be written");
    }
    offset += written;
  }
  if (!FlushFileBuffers(handle.get())) {
    fail("staging_failed", "private staged file could not be flushed");
  }
}

[[nodiscard]] FpControlPair capture_fp_control() noexcept {
  FpControlPair value;
  mm108_store_x87_control_word(&value.x87_control_word);
  mm108_store_mxcsr(&value.mxcsr);
  return value;
}

class FpEnvironmentGuard final {
 public:
  explicit FpEnvironmentGuard(FpEnvironmentRecord &record) noexcept
      : record_(record) {
    record_.before = capture_fp_control();
    mm108_load_x87_control_word(kAppliedX87ControlWord);
    mm108_load_mxcsr(kAppliedMxcsr);
    record_.applied = capture_fp_control();
  }

  ~FpEnvironmentGuard() {
    if (!restored_) {
      mm108_load_x87_control_word(record_.before.x87_control_word);
      mm108_load_mxcsr(record_.before.mxcsr);
    }
  }

  FpEnvironmentGuard(const FpEnvironmentGuard &) = delete;
  FpEnvironmentGuard &operator=(const FpEnvironmentGuard &) = delete;

  void capture_after() noexcept { record_.after = capture_fp_control(); }

  void restore() noexcept {
    mm108_load_x87_control_word(record_.before.x87_control_word);
    mm108_load_mxcsr(record_.before.mxcsr);
    record_.restored = capture_fp_control();
    restored_ = true;
  }

 private:
  FpEnvironmentRecord &record_;
  bool restored_ = false;
};

[[nodiscard]] std::wstring module_path(HMODULE module) {
  std::wstring output(32768, L'\0');
  const DWORD length =
      GetModuleFileNameW(module, output.data(), static_cast<DWORD>(output.size()));
  if (length == 0 || length >= output.size()) {
    fail("loader_verification_failed", "loaded module path is unavailable");
  }
  output.resize(length);
  return output;
}

void verify_loaded_module(std::wstring_view module_name,
                          const std::wstring &expected_path,
                          const LockedFile &expected_file) {
  const std::wstring name(module_name);
  HMODULE module = GetModuleHandleW(name.c_str());
  if (module == nullptr ||
      !same_path(module_path(module), expected_path)) {
    fail("loader_verification_failed",
         "loaded runtime module did not come from private staging");
  }
  LockedFile loaded =
      read_locked_file(expected_path, kMaximumArtifactBytes,
                       "loader_verification_failed");
  if (loaded.object_identity != expected_file.object_identity) {
    fail("loader_verification_failed",
         "loaded runtime path no longer names the locked artifact");
  }
  verify_identity(loaded.identity, expected_file.identity,
                  "loader_verification_failed",
                  "loaded runtime module identity changed");
}

[[nodiscard]] bool memory_range_is_readable(const void *pointer,
                                            std::size_t byte_length) noexcept {
  if (pointer == nullptr || byte_length == 0) {
    return false;
  }
  std::uintptr_t current = reinterpret_cast<std::uintptr_t>(pointer);
  if (current > std::numeric_limits<std::uintptr_t>::max() - byte_length) {
    return false;
  }
  const std::uintptr_t end = current + byte_length;
  while (current < end) {
    MEMORY_BASIC_INFORMATION info{};
    if (VirtualQuery(reinterpret_cast<const void *>(current), &info,
                     sizeof(info)) != sizeof(info) ||
        info.State != MEM_COMMIT || (info.Protect & PAGE_GUARD) != 0 ||
        (info.Protect & PAGE_NOACCESS) != 0) {
      return false;
    }
    const std::uintptr_t region_end =
        reinterpret_cast<std::uintptr_t>(info.BaseAddress) + info.RegionSize;
    if (region_end <= current) {
      return false;
    }
    current = std::min(region_end, end);
  }
  return true;
}

void validate_executable_rva(HMODULE module, std::uint32_t rva) {
  const auto *base = reinterpret_cast<const std::byte *>(module);
  const auto *dos = reinterpret_cast<const IMAGE_DOS_HEADER *>(base);
  if (dos->e_magic != IMAGE_DOS_SIGNATURE || dos->e_lfanew <= 0) {
    fail("target_layout_mismatch", "fixed target has an invalid PE header");
  }
  const auto *nt = reinterpret_cast<const IMAGE_NT_HEADERS64 *>(
      base + static_cast<std::size_t>(dos->e_lfanew));
  if (nt->Signature != IMAGE_NT_SIGNATURE ||
      nt->FileHeader.Machine != IMAGE_FILE_MACHINE_AMD64 ||
      nt->OptionalHeader.Magic != IMAGE_NT_OPTIONAL_HDR64_MAGIC ||
      rva >= nt->OptionalHeader.SizeOfImage) {
    fail("target_layout_mismatch", "fixed target is not the expected PE64 image");
  }
  const IMAGE_SECTION_HEADER *section = IMAGE_FIRST_SECTION(nt);
  for (std::uint16_t index = 0; index < nt->FileHeader.NumberOfSections;
       ++index) {
    const std::uint32_t start = section[index].VirtualAddress;
    const std::uint32_t size =
        std::max(section[index].Misc.VirtualSize, section[index].SizeOfRawData);
    if (rva >= start && rva - start < size &&
        (section[index].Characteristics & IMAGE_SCN_MEM_EXECUTE) != 0) {
      return;
    }
  }
  fail("target_layout_mismatch", "fixed core RVA is not executable");
}

[[nodiscard]] SessionSnapshot capture_session(const std::byte *session) {
  return SessionSnapshot{
      load_unaligned<std::uint32_t>(session + 0x14),
      load_unaligned<std::uint64_t>(session + 0x18),
      load_unaligned<std::uint64_t>(session + 0x68),
  };
}

void validate_finite_f32_bits(std::uint32_t bits) {
  if (!std::isfinite(std::bit_cast<float>(bits))) {
    fail("non_finite_result", "fixed core produced a non-finite result");
  }
}

void validate_finite_f64_bits(std::uint64_t bits) {
  if (!std::isfinite(std::bit_cast<double>(bits))) {
    fail("non_finite_result", "fixed core produced non-finite channel state");
  }
}

using InitFunction = void *(__cdecl *)(void *, std::uint32_t, std::uint32_t,
                                       double);
using PushFunction = void(__cdecl *)(void *, const double *, std::uint32_t);
using FinishFunction = void(__cdecl *)(void *, void *, bool);
using CleanupFunction = void(__cdecl *)(void *);

void execute_core(HMODULE module, const Request &request,
                  const std::vector<double> &samples, CoreOutput &output,
                  FpEnvironmentGuard &fp_guard) {
  auto *base = reinterpret_cast<std::byte *>(module);
  auto init = reinterpret_cast<InitFunction>(base + kInitRva);
  auto push = reinterpret_cast<PushFunction>(base + kPushRva);
  auto finish = reinterpret_cast<FinishFunction>(base + kFinishRva);
  auto cleanup_result =
      reinterpret_cast<CleanupFunction>(base + kResultCleanupRva);
  auto cleanup_histogram =
      reinterpret_cast<CleanupFunction>(base + kHistogramCleanupRva);
  auto cleanup_channel_state =
      reinterpret_cast<CleanupFunction>(base + kChannelStateCleanupRva);
  auto cleanup_accumulator =
      reinterpret_cast<CleanupFunction>(base + kAccumulatorCleanupRva);

  alignas(16) std::array<std::byte, kSessionSize> session{};
  alignas(16) std::array<std::byte, kResultSize> result{};
  trace_checkpoint("before-init");
  if (init(session.data(), request.sample_rate, request.channels, 0.0) !=
      session.data()) {
    fail("core_contract_mismatch",
         "fixed core initialization returned an unexpected object");
  }
  trace_checkpoint("after-init");

  std::uint64_t frame_offset = 0;
  while (frame_offset < request.frames) {
    const std::uint32_t frames = static_cast<std::uint32_t>(
        std::min<std::uint64_t>(request.frames - frame_offset,
                                request.block_frames));
    const std::size_t sample_offset =
        static_cast<std::size_t>(frame_offset * request.channels);
    push(session.data(), samples.data() + sample_offset, frames);
    frame_offset += frames;
  }
  trace_checkpoint("after-push");
  output.before_finish = capture_session(session.data());
  if (output.before_finish.submitted_frames +
          output.before_finish.current_window_frames !=
      request.frames) {
    fail("core_contract_mismatch",
         "fixed core pre-finish frame accounting is inconsistent");
  }

  finish(session.data(), result.data(), false);
  trace_checkpoint("after-finish");
  output.after_finish = capture_session(session.data());
  if (output.after_finish.current_window_frames != 0 ||
      output.after_finish.submitted_frames != request.frames) {
    fail("core_contract_mismatch",
         "fixed core finalized frame accounting is inconsistent");
  }
  const std::uint64_t expected_window_delta =
      output.before_finish.current_window_frames == 0 ? 0 : 1;
  if (output.after_finish.window_count !=
      output.before_finish.window_count + expected_window_delta) {
    fail("core_contract_mismatch",
         "fixed core finalized window accounting is inconsistent");
  }

  const auto channel_state_pointer =
      load_unaligned<std::uintptr_t>(session.data() + 0x38);
  const std::size_t channel_state_bytes =
      static_cast<std::size_t>(request.channels) * kChannelStateStride;
  if (!memory_range_is_readable(
          reinterpret_cast<const void *>(channel_state_pointer),
          channel_state_bytes)) {
    fail("core_contract_mismatch", "fixed core channel-state storage is invalid");
  }
  output.channel_state.reserve(request.channels);
  const auto *state = reinterpret_cast<const std::byte *>(channel_state_pointer);
  for (std::uint32_t channel = 0; channel < request.channels; ++channel) {
    const std::byte *item =
        state + static_cast<std::size_t>(channel) * kChannelStateStride;
    ChannelState captured{
        load_unaligned<std::uint64_t>(item + 0x00),
        load_unaligned<std::uint64_t>(item + 0x08),
        load_unaligned<std::uint64_t>(item + 0x10),
        load_unaligned<std::uint64_t>(item + 0x18),
        load_unaligned<std::uint64_t>(item + 0x20),
    };
    validate_finite_f64_bits(captured.rms_square_sum_bits);
    validate_finite_f64_bits(captured.primary_peak_bits);
    validate_finite_f64_bits(captured.secondary_peak_bits);
    validate_finite_f64_bits(captured.primary_peak_key_bits);
    validate_finite_f64_bits(captured.secondary_peak_key_bits);
    output.channel_state.push_back(captured);
  }

  const std::uint32_t result_channels =
      load_unaligned<std::uint32_t>(result.data() + 0x0C);
  const std::uint32_t result_sample_rate =
      load_unaligned<std::uint32_t>(result.data() + 0x14);
  const std::uint64_t result_frames =
      load_unaligned<std::uint64_t>(result.data() + 0x20);
  if (result_channels != request.channels ||
      result_sample_rate != request.sample_rate ||
      result_frames != request.frames) {
    fail("core_contract_mismatch", "fixed core result geometry is inconsistent");
  }
  output.track_dr_bits = load_unaligned<std::uint32_t>(result.data() + 0x00);
  validate_finite_f32_bits(output.track_dr_bits);

  const auto dr_pointer =
      load_unaligned<std::uintptr_t>(result.data() + 0x28);
  const auto peak_pointer =
      load_unaligned<std::uintptr_t>(result.data() + 0x38);
  const auto rms_pointer =
      load_unaligned<std::uintptr_t>(result.data() + 0x48);
  const std::size_t array_bytes =
      static_cast<std::size_t>(request.channels) * sizeof(std::uint32_t);
  if (!memory_range_is_readable(reinterpret_cast<const void *>(dr_pointer),
                                array_bytes) ||
      !memory_range_is_readable(reinterpret_cast<const void *>(peak_pointer),
                                array_bytes) ||
      !memory_range_is_readable(reinterpret_cast<const void *>(rms_pointer),
                                array_bytes)) {
    fail("core_contract_mismatch", "fixed core result arrays are invalid");
  }
  output.channel_results.reserve(request.channels);
  for (std::uint32_t channel = 0; channel < request.channels; ++channel) {
    ChannelResult captured{
        load_unaligned<std::uint32_t>(
            reinterpret_cast<const std::byte *>(dr_pointer) + channel * 4),
        load_unaligned<std::uint32_t>(
            reinterpret_cast<const std::byte *>(peak_pointer) + channel * 4),
        load_unaligned<std::uint32_t>(
            reinterpret_cast<const std::byte *>(rms_pointer) + channel * 4),
    };
    validate_finite_f32_bits(captured.dr_bits);
    validate_finite_f32_bits(captured.peak_bits);
    validate_finite_f32_bits(captured.rms_bits);
    output.channel_results.push_back(captured);
  }
  fp_guard.capture_after();

  cleanup_result(result.data());
  cleanup_histogram(session.data() + 0x50);
  cleanup_channel_state(session.data() + 0x38);
  cleanup_accumulator(session.data() + 0x20);
  trace_checkpoint("after-cleanup");
}

[[nodiscard]] std::string render_fp_pair(const FpControlPair &value) {
  return "{\"x87ControlWordBits\":" +
         json_escape(fixed_hex(value.x87_control_word)) +
         ",\"mxcsrBits\":" + json_escape(fixed_hex(value.mxcsr)) + "}";
}

[[nodiscard]] std::string render_session(const SessionSnapshot &value) {
  return "{\"currentWindowFrames\":" + decimal(value.current_window_frames) +
         ",\"windowCount\":" + decimal(value.window_count) +
         ",\"submittedFrames\":" + decimal(value.submitted_frames) + "}";
}

[[nodiscard]] std::string render_result(const ResponseContext &context,
                                        const Request &request,
                                        const CoreOutput &output) {
  std::string json =
      "{\"schemaVersion\":1,\"kind\":\"foo_dr_meter_108_core_result\","
      "\"requestId\":" +
      json_escape(context.request_id) +
      ",\"targetSha256\":" + json_escape(context.target_sha256) + ",\"data\":{";
  json += "\"sampleRateHz\":" + decimal(request.sample_rate);
  json += ",\"channels\":" + decimal(request.channels);
  json += ",\"frames\":" + decimal(request.frames);
  json += ",\"trackDrBits\":" +
          json_escape(fixed_hex(output.track_dr_bits));
  json += ",\"channelResults\":[";
  for (std::size_t index = 0; index < output.channel_results.size(); ++index) {
    if (index != 0) {
      json.push_back(',');
    }
    const ChannelResult &item = output.channel_results[index];
    json += "{\"index\":" + decimal(index) +
            ",\"drBits\":" + json_escape(fixed_hex(item.dr_bits)) +
            ",\"peakBits\":" + json_escape(fixed_hex(item.peak_bits)) +
            ",\"rmsBits\":" + json_escape(fixed_hex(item.rms_bits)) + "}";
  }
  json += "],\"runtimeArtifacts\":[";
  for (std::size_t index = 0; index < request.runtime_artifacts.size(); ++index) {
    if (index != 0) {
      json.push_back(',');
    }
    const RuntimeArtifact &item = request.runtime_artifacts[index];
    json += "{\"name\":" + json_escape(item.name) +
            ",\"sha256\":" + json_escape(item.identity.sha256) +
            ",\"byteLength\":" + decimal(item.identity.byte_length) + "}";
  }
  json +=
      "],\"loaderMode\":\"private_staging_dll_load_dir_system32\","
      "\"sharedServiceBoundary\":{\"loadLifecycle\":\"real_shared\","
      "\"coreExecution\":\"fail_fast_iat_tripwire\","
      "\"armedImportCount\":13},"
      "\"sessionBeforeFinish\":" +
      render_session(output.before_finish) +
      ",\"sessionAfterFinish\":" + render_session(output.after_finish);
  json += ",\"channelStateAfterFinish\":[";
  for (std::size_t index = 0; index < output.channel_state.size(); ++index) {
    if (index != 0) {
      json.push_back(',');
    }
    const ChannelState &item = output.channel_state[index];
    json += "{\"index\":" + decimal(index) +
            ",\"rmsSquareSumBits\":" +
            json_escape(fixed_hex(item.rms_square_sum_bits)) +
            ",\"primaryPeakBits\":" +
            json_escape(fixed_hex(item.primary_peak_bits)) +
            ",\"secondaryPeakBits\":" +
            json_escape(fixed_hex(item.secondary_peak_bits)) +
            ",\"primaryPeakKeyBits\":" +
            json_escape(fixed_hex(item.primary_peak_key_bits)) +
            ",\"secondaryPeakKeyBits\":" +
            json_escape(fixed_hex(item.secondary_peak_key_bits)) + "}";
  }
  const FpEnvironmentRecord &fp = output.fp_environment;
  json += "],\"fpEnvironment\":{\"before\":" + render_fp_pair(fp.before);
  json += ",\"applied\":{\"x87ControlWordBits\":" +
          json_escape(fixed_hex(fp.applied.x87_control_word)) +
          ",\"mxcsrBits\":" + json_escape(fixed_hex(fp.applied.mxcsr)) +
          ",\"rounding\":\"nearest\",\"ftz\":false,\"daz\":false,"
          "\"exceptionsMasked\":true}";
  json += ",\"after\":" + render_fp_pair(fp.after);
  json += ",\"restored\":" + render_fp_pair(fp.restored) + "}}}\n";
  return json;
}

[[nodiscard]] std::string render_error(const ResponseContext &context,
                                       std::string_view code,
                                       std::string_view message) {
  return "{\"schemaVersion\":1,\"kind\":\"foo_dr_meter_108_core_error\","
         "\"requestId\":" +
         json_escape(context.request_id) +
         ",\"targetSha256\":" + json_escape(context.target_sha256) +
         ",\"error\":{\"code\":" + json_escape(code) +
         ",\"message\":" + json_escape(message) + "}}\n";
}

[[nodiscard]] LockedFile read_request_file(const std::wstring &path) {
  return read_locked_file(path, kMaximumRequestBytes, "invalid_request");
}

[[nodiscard]] std::wstring require_request_argument(int argc,
                                                    wchar_t **argv) {
  if (argc != 3 || std::wstring_view(argv[1]) != L"--request" ||
      argv[2][0] == L'\0') {
    fail("invalid_arguments", "expected exactly --request REQUEST.json");
  }
  return full_path(argv[2]);
}

[[nodiscard]] int run_worker(int argc, wchar_t **argv,
                             ResponseContext &context) {
  const std::wstring request_path = require_request_argument(argc, argv);
  LockedFile request_file = read_request_file(request_path);
  const std::string_view request_text(
      reinterpret_cast<const char *>(request_file.bytes.data()),
      request_file.bytes.size());
  Request request = parse_request(request_text, context);

  LockedFile target =
      read_locked_file(request.target_path, UINT64_C(16) << 20,
                       "target_identity_mismatch");
  verify_identity(target.identity, request.target_identity,
                  "target_identity_mismatch",
                  "fixed target bytes differ from the request");

  std::vector<LockedFile> runtime_sources;
  runtime_sources.reserve(request.runtime_artifacts.size());
  for (const RuntimeArtifact &artifact : request.runtime_artifacts) {
    LockedFile source = read_locked_file(
        artifact.source_path, kMaximumArtifactBytes,
        "runtime_identity_mismatch");
    verify_identity(source.identity, artifact.identity,
                    "runtime_identity_mismatch",
                    "runtime artifact bytes differ from the request");
    runtime_sources.push_back(std::move(source));
  }

  LockedFile pcm = read_locked_file(request.pcm_path, kMaximumPcmBytes,
                                    "pcm_identity_mismatch");
  verify_identity(pcm.identity, request.pcm_identity, "pcm_identity_mismatch",
                  "PCM bytes differ from the request");
  std::vector<double> samples = decode_pcm(pcm.bytes);

  StageDirectory stage;
  const std::wstring target_stage = stage.file_path(L"foo_dr_meter.dll");
  stage.remember(target_stage);
  write_staged_file(target_stage, target.bytes);
  LockedFile staged_target =
      read_locked_file(target_stage, UINT64_C(16) << 20, "staging_failed");
  verify_identity(staged_target.identity, request.target_identity,
                  "staging_failed", "staged target identity is incorrect");

  std::vector<std::wstring> runtime_stage_paths;
  std::vector<LockedFile> staged_runtime;
  runtime_stage_paths.reserve(request.runtime_artifacts.size());
  staged_runtime.reserve(request.runtime_artifacts.size());
  for (std::size_t index = 0; index < request.runtime_artifacts.size(); ++index) {
    const std::wstring name = utf8_to_wide(request.runtime_artifacts[index].name);
    std::wstring path = stage.file_path(name);
    stage.remember(path);
    write_staged_file(path, runtime_sources[index].bytes);
    LockedFile staged =
        read_locked_file(path, kMaximumArtifactBytes, "staging_failed");
    verify_identity(staged.identity, request.runtime_artifacts[index].identity,
                    "staging_failed",
                    "staged runtime artifact identity is incorrect");
    runtime_stage_paths.push_back(std::move(path));
    staged_runtime.push_back(std::move(staged));
  }

  for (const std::string_view name : kRuntimeNames) {
    const std::wstring wide_name = utf8_to_wide(name);
    if (GetModuleHandleW(wide_name.c_str()) != nullptr) {
      fail("loader_precondition_failed",
           "allowlisted runtime module was already loaded");
    }
  }
  if (GetModuleHandleW(L"foo_dr_meter.dll") != nullptr) {
    fail("loader_precondition_failed",
         "fixed target module was already loaded");
  }
  if (!SetDefaultDllDirectories(LOAD_LIBRARY_SEARCH_SYSTEM32)) {
    fail("loader_failed", "safe DLL search policy could not be installed");
  }
  stage.verify_path_identity();
  LockedFile target_by_path =
      read_locked_file(target_stage, UINT64_C(16) << 20, "staging_failed");
  if (target_by_path.object_identity != staged_target.object_identity) {
    fail("staging_failed",
         "staged target path no longer names the locked artifact");
  }
  verify_identity(target_by_path.identity, staged_target.identity,
                  "staging_failed",
                  "staged target identity changed before loading");
  for (std::size_t index = 0; index < staged_runtime.size(); ++index) {
    LockedFile runtime_by_path =
        read_locked_file(runtime_stage_paths[index], kMaximumArtifactBytes,
                         "staging_failed");
    if (runtime_by_path.object_identity !=
        staged_runtime[index].object_identity) {
      fail("staging_failed",
           "staged runtime path no longer names the locked artifact");
    }
    verify_identity(runtime_by_path.identity, staged_runtime[index].identity,
                    "staging_failed",
                    "staged runtime identity changed before loading");
  }
  trace_checkpoint("before-load");
  UniqueModule module(LoadLibraryExW(
      target_stage.c_str(), nullptr,
      LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_SYSTEM32));
  if (module.get() == nullptr) {
    fail("loader_failed", "fixed target could not be privately loaded");
  }
  trace_checkpoint("after-load");
  stage.verify_path_identity();
  if (!same_path(module_path(module.get()), target_stage)) {
    fail("loader_verification_failed",
         "loaded target did not come from private staging");
  }
  LockedFile loaded_target =
      read_locked_file(target_stage, UINT64_C(16) << 20,
                       "loader_verification_failed");
  if (loaded_target.object_identity != staged_target.object_identity) {
    fail("loader_verification_failed",
         "loaded target path no longer names the locked artifact");
  }
  verify_identity(loaded_target.identity, staged_target.identity,
                  "loader_verification_failed",
                  "loaded target module identity changed");
  for (std::size_t index = 0; index < request.runtime_artifacts.size(); ++index) {
    verify_loaded_module(utf8_to_wide(request.runtime_artifacts[index].name),
                         runtime_stage_paths[index],
                         staged_runtime[index]);
  }
  trace_checkpoint("after-module-verification");

  HMODULE shared = GetModuleHandleW(L"shared.dll");
  if (shared == nullptr) {
    fail("loader_verification_failed", "shared runtime was not loaded");
  }
  FARPROC marker =
      GetProcAddress(shared, "macinmeter_shared_shim_marker_v1");
  if (request.runtime_profile == "fail_fast_shared_v1") {
    if (marker == nullptr) {
      fail("loader_verification_failed",
           "fail-fast shared runtime marker is absent");
    }
    using MarkerFunction = std::uint32_t(__cdecl *)();
    if (reinterpret_cast<MarkerFunction>(marker)() != kSharedShimMarker) {
      fail("loader_verification_failed",
           "fail-fast shared runtime marker is incorrect");
    }
    trace_checkpoint("after-shim-marker");
  } else if (marker != nullptr) {
    fail("loader_verification_failed",
         "real runtime unexpectedly contains the shim marker");
  }

  for (const std::uint32_t rva :
       {kInitRva, kPushRva, kFinishRva, kResultCleanupRva,
        kHistogramCleanupRva, kChannelStateCleanupRva,
        kAccumulatorCleanupRva}) {
    validate_executable_rva(module.get(), rva);
  }

  if (request.runtime_profile != "fixed_foobar_2_25_10") {
    fail("runtime_profile_not_executable",
         "fail-fast shared is a load-lifecycle negative probe only");
  }
  shared_iat::Error tripwire_error{};
  std::optional<shared_iat::Tripwire> tripwire =
      shared_iat::Tripwire::arm(
          module.get(),
          reinterpret_cast<std::uintptr_t>(&macinmeter_shared_core_tripwire),
          tripwire_error);
  if (!tripwire.has_value() || tripwire->import_count() != 13) {
    fail("shared_boundary_failed",
         "fixed shared-service import boundary could not be armed");
  }
  trace_checkpoint("after-core-tripwire-arm");
  if (environment_is_one(L"MACINMETER_CORE_TRIPWIRE_SELF_TEST")) {
    using TripwireTestFunction = void(__fastcall *)();
    const std::uintptr_t target =
        tripwire->first_patched_target_for_test();
    if (target == 0) {
      fail("shared_boundary_failed",
           "fixed shared-service tripwire self-test could not start");
    }
    reinterpret_cast<TripwireTestFunction>(target)();
    fail("shared_boundary_failed",
         "fixed shared-service tripwire unexpectedly returned");
  }

  CoreOutput output;
  FpEnvironmentGuard fp_guard(output.fp_environment);
  execute_core(module.get(), request, samples, output, fp_guard);
  if (!tripwire->restore()) {
    fail("shared_boundary_failed",
         "fixed shared-service import boundary could not be restored");
  }
  trace_checkpoint("after-core-tripwire-restore");
  if (!module.unload()) {
    fail("loader_unload_failed", "fixed target could not be unloaded");
  }
  stage.verify_path_identity();
  for (const std::string_view name : kRuntimeNames) {
    const std::wstring wide_name = utf8_to_wide(name);
    if (GetModuleHandleW(wide_name.c_str()) != nullptr) {
      fail("loader_unload_failed", "private runtime module remained loaded");
    }
  }
  fp_guard.restore();
  trace_checkpoint("after-unload");
  const std::string response = render_result(context, request, output);
  write_all(GetStdHandle(STD_OUTPUT_HANDLE), response);
  return 0;
}

}  // namespace

int wmain(int argc, wchar_t **argv) {
  ResponseContext context;
  try {
    return run_worker(argc, argv, context);
  } catch (const WorkerError &error) {
    try {
      const std::string response =
          render_error(context, error.code(), error.what());
      write_all(GetStdHandle(STD_OUTPUT_HANDLE), response);
    } catch (...) {
      return 3;
    }
    return 2;
  } catch (const std::bad_alloc &) {
    try {
      const std::string response =
          render_error(context, "resource_limit", "worker allocation failed");
      write_all(GetStdHandle(STD_OUTPUT_HANDLE), response);
    } catch (...) {
      return 3;
    }
    return 2;
  }
}
