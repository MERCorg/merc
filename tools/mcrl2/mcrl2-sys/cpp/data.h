/// Wrapper around the atermpp library of the mCRL2 toolset.

#pragma once

#include "mcrl2/atermpp/aterm.h"
#include "mcrl2/atermpp/aterm_string.h"
#include "mcrl2/data/data_expression.h"
#include "mcrl2/data/sort_expression.h"
#include "mcrl2/data/variable.h"

#include "rust/cxx.h"

namespace mcrl2::data
{

inline
rust::String mcrl2_variable_to_string(const variable& var)
{
    std::stringstream ss;
    ss << var;
    return ss.str();
}

inline
bool mcrl2_is_variable(const atermpp::aterm& term)
{
    return data::is_variable(term);
}

inline
std::unique_ptr<sort_expression> mcrl2_variable_sort(const variable& var)
{
  return std::make_unique<sort_expression>(var.sort());
}

inline
std::unique_ptr<atermpp::aterm_string> mcrl2_variable_name(const variable& var)
{
  return std::make_unique<atermpp::aterm_string>(var.name());
}

std::unique_ptr<data_expression> mcrl2_data_expression();

} // namespace mcrl2::data