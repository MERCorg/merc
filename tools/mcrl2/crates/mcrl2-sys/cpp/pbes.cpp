#include "mcrl2-sys/cpp/pbes.h"
#include "mcrl2-sys/src/pbes.rs.h"
#include <cstddef>
#include <optional>

namespace mcrl2::pbes_system {
    
void mcrl2_local_control_flow_graph_vertex_outgoing_edges(std::vector<vertex_outgoing_edge>& result,
    const detail::local_control_flow_graph_vertex& vertex)
{
  for (const auto& edge : vertex.outgoing_edges())
  {
    vertex_outgoing_edge voe;
    voe.vertex = edge.first;
    voe.edges = std::make_unique<std::vector<std::size_t>>();
    for (const auto& e : edge.second)
    {
      voe.edges->emplace_back(e);
    }
    result.emplace_back(std::move(voe));
  }
}

void mcrl2_local_control_flow_graph_vertex_incoming_edges(std::vector<vertex_outgoing_edge>& result,
    const detail::local_control_flow_graph_vertex& vertex)
{
  for (const auto& edge : vertex.incoming_edges())
  {
    vertex_outgoing_edge voe;
    voe.vertex = edge.first;
    voe.edges = std::make_unique<std::vector<std::size_t>>();
    for (const auto& e : edge.second)
    {
      voe.edges->emplace_back(e);
    }
    result.emplace_back(std::move(voe));
  }
}

std::unique_ptr<atermpp::aterm> mcrl2_pbes_expression_replace_variables(const atermpp::aterm& expr, const rust::Vec<assignment_pair>& sigma) 
{    
  MCRL2_ASSERT(is_pbes_expression(expr));

  data::mutable_map_substitution<> tmp;
  // for (const auto& assign : sigma) 
  // {
  //     tmp[atermpp::assign_cast<data::variable>(*(assign.lhs))] = atermpp::down_cast<data::data_expression>(*(assign.rhs));
  // }

  // return std::make_unique<atermpp::aterm>(pbes_system::replace_variables(atermpp::assign_cast<pbes_expression>(expr), tmp));
  return std::nullptr_t{};
}

}