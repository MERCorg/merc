/// Wrapper around the atermpp library of the mCRL2 toolset.

#pragma once

#include "mcrl2/atermpp/aterm.h"
#include "mcrl2/atermpp/aterm_list.h"
#include "mcrl2/atermpp/aterm_string.h"

#include "rust/cxx.h"

#include <cstddef>
#include <memory>

namespace atermpp
{

// atermpp::aterm_list

inline
std::unique_ptr<aterm> mcrl2_aterm_list_front(const aterm_list& term)
{
  return std::make_unique<aterm>(term.front());
}

inline
std::unique_ptr<aterm> mcrl2_aterm_list_tail(const aterm_list& term)
{
  return std::make_unique<aterm>(term.tail());
}

inline
std::unique_ptr<aterm> mcrl2_aterm_argument(const aterm& term, std::size_t index)
{
  return std::make_unique<aterm>(term[index]);
}

std::unique_ptr<aterm_list> mcrl2_aterm_list();

// atermpp::aterm

inline
rust::String mcrl2_aterm_to_string(const aterm& term)
{
    std::stringstream ss;
    ss << term;
    return ss.str();
}

// atermpp::aterm_string

inline
rust::String mcrl2_aterm_string_to_string(const aterm_string& term)
{
    std::stringstream ss;
    ss << term;
    return ss.str();
}

std::unique_ptr<aterm_string> mcrl2_aterm_string();

} // namespace atermpp