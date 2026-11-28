// Exact boolean of planar meshes (CGAL Nef polyhedra, exact constructions over GMP rationals):
// the oracle for booleans whose every face is flat.
//
//   cgal_bool <Union|Subtract|Intersect> target.off tool.off [tool.off ...]
// prints the result's volume (exact, then rounded to a double).
#include <CGAL/Exact_predicates_exact_constructions_kernel.h>
#include <CGAL/Nef_polyhedron_3.h>
#include <CGAL/Surface_mesh.h>
#include <CGAL/boost/graph/convert_nef_polyhedron_to_polygon_mesh.h>
#include <CGAL/Polygon_mesh_processing/measure.h>
#include <CGAL/Polygon_mesh_processing/triangulate_faces.h>
#include <CGAL/IO/polygon_mesh_io.h>
#include <CGAL/Polygon_mesh_processing/repair_polygon_soup.h>
#include <CGAL/Polygon_mesh_processing/orient_polygon_soup.h>
#include <CGAL/Polygon_mesh_processing/polygon_soup_to_polygon_mesh.h>
#include <CGAL/IO/polygon_soup_io.h>
#include <iostream>
#include <string>
#include <vector>

typedef CGAL::Exact_predicates_exact_constructions_kernel K;
typedef CGAL::Surface_mesh<K::Point_3> Mesh;
typedef CGAL::Nef_polyhedron_3<K> Nef;
namespace PMP = CGAL::Polygon_mesh_processing;

static Nef load(const char* path) {
    std::vector<K::Point_3> pts;
    std::vector<std::vector<std::size_t>> polys;
    if (!CGAL::IO::read_polygon_soup(path, pts, polys)) {
        std::cerr << "cannot read " << path << "\n";
        std::exit(2);
    }
    PMP::repair_polygon_soup(pts, polys);
    PMP::orient_polygon_soup(pts, polys);
    Mesh m;
    PMP::polygon_soup_to_polygon_mesh(pts, polys, m);
    if (!CGAL::is_closed(m)) {
        std::cerr << "not closed: " << path << "\n";
        std::exit(3);
    }
    return Nef(m);
}

int main(int argc, char** argv) {
    if (argc < 4) {
        std::cerr << "usage: cgal_bool op target.off tool.off...\n";
        return 1;
    }
    std::string op = argv[1];
    Nef a = load(argv[2]);
    Nef tools = load(argv[3]);
    for (int i = 4; i < argc; ++i) tools += load(argv[i]);
    Nef r = op == "Union" ? a + tools : op == "Subtract" ? a - tools : a * tools;
    Mesh out;
    CGAL::convert_nef_polyhedron_to_polygon_mesh(r, out, true);
    K::FT v = out.number_of_faces() ? PMP::volume(out) : K::FT(0);
    std::cout.precision(17);
    std::cout << CGAL::to_double(v) << "\n";
    return 0;
}
