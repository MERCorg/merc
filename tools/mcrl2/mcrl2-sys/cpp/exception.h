
#include <cpptrace/from_current.hpp>

#include <cstdlib>

namespace rust::behavior {

template <typename Try, typename Fail>
static void trycatch(Try &&func, Fail &&fail) noexcept 
{ 
  CPPTRACE_TRY {
    func();
  } CPPTRACE_CATCH(const std::exception &e) {
    if (std::getenv("RUST_BACKTRACE") != nullptr) {
      cpptrace::from_current_exception().print();
    }

    fail(e.what());
  }
}

} // namespace rust::behaviour