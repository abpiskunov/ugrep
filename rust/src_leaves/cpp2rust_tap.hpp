// cpp2rust_tap.hpp - seam tap for the C++ side. TEMPLATE: copy to migration/harness/<component>/.
// Emits cpp2rust-trace/1 events (see references/trace-schema.md) as JSON Lines.
// Everything compiles to nothing unless CPP2RUST_TAP is defined, so product builds are unchanged.
// Use only in generated facade/bridge files, never in business code.
//
//   int Facade::parse_header(const std::string& path, Header* out) {
//     TAP_CALL("core_parse.parse_header", path);            // inbound call + neutral args
//     int rc = legacy::parse_header(path, out);
//     TAP_STATE(*out);                                       // neutral snapshot after the call
//     TAP_RET(rc);
//     return rc;
//   }
//   // outbound call to a dependency that stays C++ (also the fault-injection point):
//   TAP_OUT("fs.read", path);
//   if (auto e = TAP_FAULT("fs.read")) { TAP_OUT_ERR("fs.read", *e); return make_error(*e); }
//   auto n = fs::read(path, buf);  TAP_OUT_RET("fs.read", n);
//
// Environment: CPP2RUST_TAP_OUT=<file> (required to record), CPP2RUST_TAP_CASE=<case id>,
//              CPP2RUST_FAULT=<callee>:<n>:<error>  (the n-th call to callee fails with error; n from 1)
// Your own types: define `std::string to_neutral(const YourType&)` in the type's namespace (found
// by argument-dependent lookup), inside `#ifdef CPP2RUST_TAP`, in the facade file.
#pragma once
#ifdef CPP2RUST_TAP
#include <cstdio>
#include <cstdlib>
#include <map>
#include <mutex>
#include <optional>
#include <string>
#include <thread>
#include <type_traits>
#include <vector>

namespace cpp2rust::tap {

inline std::string quote(const std::string& s) {
  std::string o = "\"";
  for (unsigned char c : s) {
    if (c == '"' || c == '\\') { o += '\\'; o += static_cast<char>(c); }
    else if (c < 0x20 || c >= 0x7f) { char b[8]; std::snprintf(b, sizeof b, "\\u%04x", c); o += b; }  // bytes, not UTF-8
    else o += static_cast<char>(c);
  }
  return o + "\"";
}

// Neutral encoders. Integers and bools as JSON; floats as %.17g text (bit-faithful, NaN-safe);
// strings as byte-escaped JSON strings; pointers never by address (use obj_id).
inline std::string to_neutral(bool v) { return v ? "true" : "false"; }
inline std::string to_neutral(const std::string& v) { return quote(v); }
inline std::string to_neutral(const char* v) { return v ? quote(v) : "null"; }
template <class T, std::enable_if_t<std::is_integral_v<T> && !std::is_same_v<T, bool>, int> = 0>
std::string to_neutral(T v) { return std::to_string(v); }
template <class T, std::enable_if_t<std::is_floating_point_v<T>, int> = 0>
std::string to_neutral(T v) { char b[40]; std::snprintf(b, sizeof b, "\"%.17g\"", static_cast<double>(v)); return b; }
template <class T, std::enable_if_t<std::is_enum_v<T>, int> = 0>
std::string to_neutral(T v) { return std::to_string(static_cast<long long>(v)); }
template <class T> std::string to_neutral(const std::optional<T>& v);
template <class T> std::string to_neutral(const std::vector<T>& v);
template <class K, class V> std::string to_neutral(const std::map<K, V>& m);
template <class T> std::string to_neutral(const std::optional<T>& v) { return v ? to_neutral(*v) : "null"; }
template <class T> std::string to_neutral(const std::vector<T>& v) {
  std::string o = "[";
  for (size_t i = 0; i < v.size(); ++i) { if (i) o += ","; o += to_neutral(v[i]); }
  return o + "]";
}
template <class K, class V> std::string to_neutral(const std::map<K, V>& m) {  // ordered: stable
  std::string o = "[";
  bool first = true;
  for (const auto& [k, v] : m) { if (!first) o += ","; first = false; o += "[" + to_neutral(k) + "," + to_neutral(v) + "]"; }
  return o + "]";
}
template <class T> std::string neutral(const T& v) { return to_neutral(v); }  // entry point for the macros
inline std::string args() { return "[]"; }
template <class... A> std::string args(const A&... a) {
  std::string o = "[";
  bool first = true;
  ((o += (first ? "" : ","), o += neutral(a), first = false), ...);
  return o + "]";
}

class Sink {
 public:
  static Sink& get() { static Sink s; return s; }
  void set_case(const std::string& c) { std::lock_guard<std::mutex> l(mu_); case_ = c; seq_.clear(); objs_.clear(); counts_.clear(); }
  // Object identity without addresses: ids in first-seen order, per case.
  std::string obj_id(const void* p) {
    std::lock_guard<std::mutex> l(mu_);
    auto it = objs_.find(p);
    if (it == objs_.end()) it = objs_.emplace(p, objs_.size() + 1).first;
    return "\"#" + std::to_string(it->second) + "\"";
  }
  void emit(const char* kind, const std::string& fn, const std::string& fields, const std::string& in) {
    if (!f_) return;
    std::lock_guard<std::mutex> l(mu_);
    int th = thread_id();
    std::fprintf(f_, "{\"impl\":\"cpp\",\"case\":%s,\"thread\":%d,\"seq\":%ld,\"kind\":\"%s\",\"fn\":%s%s%s%s}\n",
                 quote(case_).c_str(), th, seq_[th]++, kind, quote(fn).c_str(),
                 in.empty() ? "" : (",\"in\":" + quote(in)).c_str(), fields.empty() ? "" : ",", fields.c_str());
    std::fflush(f_);
  }
  // Fault schedule: returns the error to inject when this is the scheduled call.
  std::optional<std::string> fault(const std::string& callee) {
    std::lock_guard<std::mutex> l(mu_);
    long n = ++counts_[callee];
    if (callee == fault_fn_ && n == fault_n_) return fault_err_;
    return std::nullopt;
  }

 private:
  Sink() {
    if (const char* p = std::getenv("CPP2RUST_TAP_OUT")) f_ = std::fopen(p, "a");
    if (const char* c = std::getenv("CPP2RUST_TAP_CASE")) case_ = c;
    if (const char* s = std::getenv("CPP2RUST_FAULT")) {
      std::string v = s;
      auto a = v.find(':'), b = v.find(':', a + 1);
      if (a != std::string::npos && b != std::string::npos) {
        fault_fn_ = v.substr(0, a); fault_n_ = std::atol(v.substr(a + 1, b - a - 1).c_str()); fault_err_ = v.substr(b + 1);
      }
    }
  }
  int thread_id() {  // logical ids in first-seen order
    auto id = std::this_thread::get_id();
    auto it = threads_.find(id);
    if (it == threads_.end()) it = threads_.emplace(id, static_cast<int>(threads_.size())).first;
    return it->second;
  }
  std::FILE* f_ = nullptr;
  std::mutex mu_;
  std::string case_ = "default", fault_fn_, fault_err_;
  long fault_n_ = 0;
  std::map<int, long> seq_;
  std::map<const void*, size_t> objs_;
  std::map<std::thread::id, int> threads_;
  std::map<std::string, long> counts_;
};

inline thread_local std::string current_in;  // enclosing inbound seam function

// One inbound call. If the function leaves by exception without TAP_RET/TAP_ERR, the
// destructor records err="exception" so both sides show how the call ended.
class Call {
 public:
  Call(const char* fn, const std::string& a) : fn_(fn), prev_(current_in) {
    current_in = fn_;
    Sink::get().emit("call", fn_, "\"args\":" + a, fn_);
  }
  void state(const std::string& s) { Sink::get().emit("state", fn_, "\"state\":" + s, fn_); }
  void ret(const std::string& r) { done_ = true; Sink::get().emit("ret", fn_, "\"ret\":" + r, fn_); }
  void err(const std::string& e) { done_ = true; Sink::get().emit("ret", fn_, "\"err\":" + e, fn_); }
  ~Call() {
    if (!done_) Sink::get().emit("ret", fn_, std::uncaught_exceptions() ? "\"err\":\"exception\"" : "\"ret\":null", fn_);
    current_in = prev_;
  }

 private:
  std::string fn_, prev_;
  bool done_ = false;
};

}  // namespace cpp2rust::tap

#define TAP_CALL(fn, ...) ::cpp2rust::tap::Call cpp2rust_tap_call_(fn, ::cpp2rust::tap::args(__VA_ARGS__))
#define TAP_STATE(v) cpp2rust_tap_call_.state(::cpp2rust::tap::neutral(v))
#define TAP_RET(v) cpp2rust_tap_call_.ret(::cpp2rust::tap::neutral(v))
#define TAP_ERR(v) cpp2rust_tap_call_.err(::cpp2rust::tap::neutral(v))
#define TAP_OUT(callee, ...) ::cpp2rust::tap::Sink::get().emit("out_call", callee, "\"args\":" + ::cpp2rust::tap::args(__VA_ARGS__), ::cpp2rust::tap::current_in)
#define TAP_OUT_RET(callee, v) ::cpp2rust::tap::Sink::get().emit("out_ret", callee, "\"ret\":" + ::cpp2rust::tap::neutral(v), ::cpp2rust::tap::current_in)
#define TAP_OUT_ERR(callee, v) ::cpp2rust::tap::Sink::get().emit("out_ret", callee, "\"err\":" + ::cpp2rust::tap::neutral(v), ::cpp2rust::tap::current_in)
#define TAP_FAULT(callee) ::cpp2rust::tap::Sink::get().fault(callee)
#define TAP_SET_CASE(id) ::cpp2rust::tap::Sink::get().set_case(id)
#define TAP_OBJ(ptr) ::cpp2rust::tap::Sink::get().obj_id(ptr)
#else
#include <optional>
#include <string>
#define TAP_CALL(fn, ...) ((void)0)
#define TAP_STATE(v) ((void)0)
#define TAP_RET(v) ((void)0)
#define TAP_ERR(v) ((void)0)
#define TAP_OUT(callee, ...) ((void)0)
#define TAP_OUT_RET(callee, v) ((void)0)
#define TAP_OUT_ERR(callee, v) ((void)0)
#define TAP_FAULT(callee) (std::optional<std::string>())
#define TAP_SET_CASE(id) ((void)0)
#define TAP_OBJ(ptr) (std::string())
#endif
