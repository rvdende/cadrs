// Written by tools/brep_bench_sync.py from brep-bench bb2dc5f: don't edit.

pub mod boolean {
    pub mod box_minus_through_cylinder;
    #[test]
    fn box_minus_through_cylinder() {
        crate::check(
            "cases::boolean::box_minus_through_cylinder",
            "boolean/box-minus-through-cylinder",
            |k| box_minus_through_cylinder::build(k).map_err(crate::Refusal::from),
            Some((717.2566611769187, 731.9468914507713)),
        );
    }
    pub mod boxes_common;
    #[test]
    fn boxes_common() {
        crate::check(
            "cases::boolean::boxes_common",
            "boolean/boxes-common",
            |k| boxes_common::build(k).map_err(crate::Refusal::from),
            Some((250.0, 250.0)),
        );
    }
    pub mod boxes_cut;
    #[test]
    fn boxes_cut() {
        crate::check(
            "cases::boolean::boxes_cut",
            "boolean/boxes-cut",
            |k| boxes_cut::build(k).map_err(crate::Refusal::from),
            Some((750.0, 550.0)),
        );
    }
    pub mod boxes_sharing_a_face;
    #[test]
    fn boxes_sharing_a_face() {
        crate::check(
            "cases::boolean::boxes_sharing_a_face",
            "boolean/boxes-sharing-a-face",
            |k| boxes_sharing_a_face::build(k).map_err(crate::Refusal::from),
            Some((2000.0, 1000.0)),
        );
    }
    pub mod boxes_sharing_part_of_a_face;
    #[test]
    fn boxes_sharing_part_of_a_face() {
        crate::check(
            "cases::boolean::boxes_sharing_part_of_a_face",
            "boolean/boxes-sharing-part-of-a-face",
            |k| boxes_sharing_part_of_a_face::build(k).map_err(crate::Refusal::from),
            Some((2000.0, 1100.0)),
        );
    }
    pub mod boxes_union;
    #[test]
    fn boxes_union() {
        crate::check(
            "cases::boolean::boxes_union",
            "boolean/boxes-union",
            |k| boxes_union::build(k).map_err(crate::Refusal::from),
            Some((1750.0, 950.0)),
        );
    }
    pub mod cross_drilled_cube;
    #[test]
    fn cross_drilled_cube() {
        crate::check(
            "cases::boolean::cross_drilled_cube",
            "boolean/cross-drilled-cube",
            |k| cross_drilled_cube::build(k).map_err(crate::Refusal::from),
            Some((19755.260919448436, 9970.346538825268)),
        );
    }
    pub mod drilled_plate;
    #[test]
    fn drilled_plate() {
        crate::check(
            "cases::boolean::drilled_plate",
            "boolean/drilled-plate",
            |k| drilled_plate::build(k).map_err(crate::Refusal::from),
            Some((18869.026644707676, 7304.77868423386)),
        );
    }
    pub mod golf_ball;
    #[test]
    fn golf_ball() {
        crate::check(
            "cases::boolean::golf_ball",
            "boolean/golf-ball",
            |k| golf_ball::build(k).map_err(crate::Refusal::from),
            Some((4124.028109767963, 1314.8501805536118)),
        );
    }
    pub mod half_cylinder_boss_tangent;
    #[test]
    fn half_cylinder_boss_tangent() {
        crate::check(
            "cases::boolean::half_cylinder_boss_tangent",
            "boolean/half-cylinder-boss-tangent",
            |k| half_cylinder_boss_tangent::build(k).map_err(crate::Refusal::from),
            Some((1392.699081698724, 735.6194490192345)),
        );
    }
    pub mod half_torus;
    #[test]
    fn half_torus() {
        crate::check(
            "cases::boolean::half_torus",
            "boolean/half-torus",
            |k| half_torus::build(k).map_err(crate::Refusal::from),
            Some((394.78417604357435, 419.9169172722927)),
        );
    }
    pub mod hemisphere;
    #[test]
    fn hemisphere() {
        crate::check(
            "cases::boolean::hemisphere",
            "boolean/hemisphere",
            |k| hemisphere::build(k).map_err(crate::Refusal::from),
            Some((261.79938779914943, 235.61944901923448)),
        );
    }
    pub mod indexed_pattern;
    #[test]
    fn indexed_pattern() {
        crate::check(
            "cases::boolean::indexed_pattern",
            "boolean/indexed-pattern",
            |k| indexed_pattern::build(k).map_err(crate::Refusal::from),
            Some((5906.194188748813, 3367.7873246482577)),
        );
    }
    pub mod linked_tori;
    #[test]
    fn linked_tori() {
        crate::check(
            "cases::boolean::linked_tori",
            "boolean/linked-tori",
            |k| linked_tori::build(k).map_err(crate::Refusal::from),
            Some((29.608813203268074, 118.4352528130723)),
        );
    }
    pub mod napkin_ring;
    #[test]
    fn napkin_ring() {
        crate::check(
            "cases::boolean::napkin_ring",
            "boolean/napkin-ring",
            |k| napkin_ring::build(k).map_err(crate::Refusal::from),
            Some((268.082573106329, 402.1238596594935)),
        );
    }
    pub mod nested_shells;
    #[test]
    fn nested_shells() {
        crate::check(
            "cases::boolean::nested_shells",
            "boolean/nested-shells",
            |k| nested_shells::build(k).map_err(crate::Refusal::from),
            Some((2714.336052701581, 2764.601535159018)),
        );
    }
    pub mod offset_hole_sphere;
    #[test]
    fn offset_hole_sphere() {
        crate::check(
            "cases::boolean::offset_hole_sphere",
            "boolean/offset-hole-sphere",
            |k| offset_hole_sphere::build(k).map_err(crate::Refusal::from),
            Some((29240.844399008805, 6171.861947620873)),
        );
    }
    pub mod reuleaux_tetrahedron;
    #[test]
    fn reuleaux_tetrahedron() {
        crate::check(
            "cases::boolean::reuleaux_tetrahedron",
            "boolean/reuleaux-tetrahedron",
            |k| reuleaux_tetrahedron::build(k).map_err(crate::Refusal::from),
            Some((422.1577331158267, 297.5471716584401)),
        );
    }
    pub mod ring_of_balls;
    #[test]
    fn ring_of_balls() {
        crate::check(
            "cases::boolean::ring_of_balls",
            "boolean/ring-of-balls",
            |k| ring_of_balls::build(k).map_err(crate::Refusal::from),
            Some((1320.5635370580605, 1170.8697754126144)),
        );
    }
    pub mod sphere_lens;
    #[test]
    fn sphere_lens() {
        crate::check(
            "cases::boolean::sphere_lens",
            "boolean/sphere-lens",
            |k| sphere_lens::build(k).map_err(crate::Refusal::from),
            Some((108.90854532444617, 125.66370614359172)),
        );
    }
    pub mod sphere_pocket;
    #[test]
    fn sphere_pocket() {
        crate::check(
            "cases::boolean::sphere_pocket",
            "boolean/sphere-pocket",
            |k| sphere_pocket::build(k).map_err(crate::Refusal::from),
            Some((3738.2006122008506, 1678.539816339745)),
        );
    }
    pub mod spheres_union;
    #[test]
    fn spheres_union() {
        crate::check(
            "cases::boolean::spheres_union",
            "boolean/spheres-union",
            |k| spheres_union::build(k).map_err(crate::Refusal::from),
            Some((938.2890058721515, 502.6548245743669)),
        );
    }
    pub mod steinmetz_common;
    #[test]
    fn steinmetz_common() {
        crate::check(
            "cases::boolean::steinmetz_common",
            "boolean/steinmetz-common",
            |k| steinmetz_common::build(k).map_err(crate::Refusal::from),
            Some((666.6666666666666, 400.0)),
        );
    }
    pub mod steinmetz_union;
    #[test]
    fn steinmetz_union() {
        crate::check(
            "cases::boolean::steinmetz_union",
            "boolean/steinmetz-union",
            |k| steinmetz_union::build(k).map_err(crate::Refusal::from),
            Some((4045.722313718023, 1799.114857512855)),
        );
    }
    pub mod stepped_shaft;
    #[test]
    fn stepped_shaft() {
        crate::check(
            "cases::boolean::stepped_shaft",
            "boolean/stepped-shaft",
            |k| stepped_shaft::build(k).map_err(crate::Refusal::from),
            Some((1068.1415022205297, 659.7344572538566)),
        );
    }
    pub mod tori_common;
    #[test]
    fn tori_common() {
        crate::check(
            "cases::boolean::tori_common",
            "boolean/tori-common",
            |k| tori_common::build(k).map_err(crate::Refusal::from),
            Some((154.36148884166312, 263.1894506957162)),
        );
    }
    pub mod tube;
    #[test]
    fn tube() {
        crate::check(
            "cases::boolean::tube",
            "boolean/tube",
            |k| tube::build(k).map_err(crate::Refusal::from),
            Some((502.6548245743669, 603.1857894892403)),
        );
    }
    pub mod tube_flush_ends;
    #[test]
    fn tube_flush_ends() {
        crate::check(
            "cases::boolean::tube_flush_ends",
            "boolean/tube-flush-ends",
            |k| tube_flush_ends::build(k).map_err(crate::Refusal::from),
            Some((502.6548245743669, 603.1857894892403)),
        );
    }
    pub mod unequal_lens_aslant;
    #[test]
    fn unequal_lens_aslant() {
        crate::check(
            "cases::boolean::unequal_lens_aslant",
            "boolean/unequal-lens-aslant",
            |k| unequal_lens_aslant::build(k).map_err(crate::Refusal::from),
            Some((42.245011348187795, 69.39098850747226)),
        );
    }
}
pub mod chamfer {
    pub mod block_ridge;
    #[test]
    fn block_ridge() {
        crate::check(
            "cases::chamfer::block_ridge",
            "chamfer/block-ridge",
            |k| block_ridge::build(k).map_err(crate::Refusal::from),
            Some((200.0, 216.5685424949238)),
        );
    }
    pub mod box_top_asymmetric;
    #[test]
    fn box_top_asymmetric() {
        crate::check(
            "cases::chamfer::box_top_asymmetric",
            "chamfer/box-top-asymmetric",
            |k| box_top_asymmetric::build(k).map_err(crate::Refusal::from),
            Some((1142.6666666666667, 709.2198067399883)),
        );
    }
    pub mod cube_all_edges;
    #[test]
    fn cube_all_edges() {
        crate::check(
            "cases::chamfer::cube_all_edges",
            "chamfer/cube-all-edges",
            |k| cube_all_edges::build(k).map_err(crate::Refusal::from),
            Some((802.6666666666666, 447.3595659028278)),
        );
    }
    pub mod cube_all_edges_mitered;
    #[test]
    fn cube_all_edges_mitered() {
        crate::check(
            "cases::chamfer::cube_all_edges_mitered",
            "chamfer/cube-all-edges-mitered",
            |k| cube_all_edges_mitered::build(k).map_err(crate::Refusal::from),
            Some((808.0, 453.58787847867995)),
        );
    }
    pub mod cylinder_rim_to_point;
    #[test]
    fn cylinder_rim_to_point() {
        crate::check(
            "cases::chamfer::cylinder_rim_to_point",
            "chamfer/cylinder-rim-to-point",
            |k| cylinder_rim_to_point::build(k).map_err(crate::Refusal::from),
            Some((523.5987755982989, 346.69152247319363)),
        );
    }
    pub mod l_block_inside_consumed;
    #[test]
    fn l_block_inside_consumed() {
        crate::check(
            "cases::chamfer::l_block_inside_consumed",
            "chamfer/l-block-inside-consumed",
            |k| l_block_inside_consumed::build(k).map_err(crate::Refusal::from),
            Some((755.0, 509.99494936611666)),
        );
    }
    pub mod octahedron_all_edges;
    #[test]
    fn octahedron_all_edges() {
        crate::check(
            "cases::chamfer::octahedron_all_edges",
            "chamfer/octahedron-all-edges",
            |k| octahedron_all_edges::build(k).map_err(crate::Refusal::from),
            Some((917.3333333333334, 478.3802556600424)),
        );
    }
    pub mod octahedron_all_edges_mitered;
    #[test]
    fn octahedron_all_edges_mitered() {
        crate::check(
            "cases::chamfer::octahedron_all_edges_mitered",
            "chamfer/octahedron-all-edges-mitered",
            |k| octahedron_all_edges_mitered::build(k).map_err(crate::Refusal::from),
            Some((981.3333333333334, 518.1447576478596)),
        );
    }
}
pub mod draft {
    pub mod boss_on_plate;
    #[test]
    fn boss_on_plate() {
        crate::check(
            "cases::draft::boss_on_plate",
            "draft/boss-on-plate",
            |k| boss_on_plate::build(k).map_err(crate::Refusal::from),
            Some((10168.518301825494, 4532.980531504529)),
        );
    }
    pub mod box_about_its_middle;
    #[test]
    fn box_about_its_middle() {
        crate::check(
            "cases::draft::box_about_its_middle",
            "draft/box-about-its-middle",
            |k| box_about_its_middle::build(k).map_err(crate::Refusal::from),
            Some((2402.551422081851, 1123.9755492768527)),
        );
    }
    pub mod box_cross_drilled;
    #[test]
    fn box_cross_drilled() {
        crate::check(
            "cases::draft::box_cross_drilled",
            "draft/box-cross-drilled",
            |k| box_cross_drilled::build(k).map_err(crate::Refusal::from),
            Some((9508.995905883501, 3564.816490476812)),
        );
    }
    pub mod box_four_sides;
    #[test]
    fn box_four_sides() {
        crate::check(
            "cases::draft::box_four_sides",
            "draft/box-four-sides",
            |k| box_four_sides::build(k).map_err(crate::Refusal::from),
            Some((2130.2419650444463, 1034.384515466381)),
        );
    }
    pub mod box_one_side;
    #[test]
    fn box_one_side() {
        crate::check(
            "cases::draft::box_one_side",
            "draft/box-one-side",
            |k| box_one_side::build(k).map_err(crate::Refusal::from),
            Some((2294.203811574921, 1083.059257670427)),
        );
    }
    pub mod box_outward;
    #[test]
    fn box_outward() {
        crate::check(
            "cases::draft::box_outward",
            "draft/box-outward",
            |k| box_outward::build(k).map_err(crate::Refusal::from),
            Some((2690.16941161036, 1216.6282895855456)),
        );
    }
    pub mod box_top_drafted_away;
    #[test]
    fn box_top_drafted_away() {
        crate::check(
            "cases::draft::box_top_drafted_away",
            "draft/box-top-drafted-away",
            |k| box_top_drafted_away::build(k).map_err(crate::Refusal::from),
            Some((1154.7005383792516, 900.0)),
        );
    }
    pub mod cylinder_parted_both_ways;
    #[test]
    fn cylinder_parted_both_ways() {
        crate::check(
            "cases::draft::cylinder_parted_both_ways",
            "draft/cylinder-parted-both-ways",
            |k| cylinder_parted_both_ways::build(k).map_err(crate::Refusal::from),
            Some((5959.649932324679, 1789.574330861596)),
        );
    }
    pub mod hole_tapered;
    #[test]
    fn hole_tapered() {
        crate::check(
            "cases::draft::hole_tapered",
            "draft/hole-tapered",
            |k| hole_tapered::build(k).map_err(crate::Refusal::from),
            Some((15319.89828721478, 4751.804244415088)),
        );
    }
    pub mod l_block_sides;
    #[test]
    fn l_block_sides() {
        crate::check(
            "cases::draft::l_block_sides",
            "draft/l-block-sides",
            |k| l_block_sides::build(k).map_err(crate::Refusal::from),
            Some((4593.026514919725, 2034.022842053666)),
        );
    }
    pub mod pocket_walls;
    #[test]
    fn pocket_walls() {
        crate::check(
            "cases::draft::pocket_walls",
            "draft/pocket-walls",
            |k| pocket_walls::build(k).map_err(crate::Refusal::from),
            Some((14297.337390274086, 5228.479128158012)),
        );
    }
    pub mod rounded_box_sides;
    #[test]
    fn rounded_box_sides() {
        crate::check(
            "cases::draft::rounded_box_sides",
            "draft/rounded-box-sides",
            |k| rounded_box_sides::build(k).map_err(crate::Refusal::from),
            Some((5463.267400412602, 2000.7496007461252)),
        );
    }
    pub mod rounded_pocket_walls;
    #[test]
    fn rounded_pocket_walls() {
        crate::check(
            "cases::draft::rounded_pocket_walls",
            "draft/rounded-pocket-walls",
            |k| rounded_pocket_walls::build(k).map_err(crate::Refusal::from),
            Some((14466.913089757878, 5158.670564596014)),
        );
    }
}
pub mod edit {
    pub mod block_top_moved_up;
    #[test]
    fn block_top_moved_up() {
        crate::check(
            "cases::edit::block_top_moved_up",
            "edit/block-top-moved-up",
            |k| block_top_moved_up::build(k).map_err(crate::Refusal::from),
            Some((17943.36293856408, 4985.575191894877)),
        );
    }
    pub mod hole_widened;
    #[test]
    fn hole_widened() {
        crate::check(
            "cases::edit::hole_widened",
            "edit/hole-widened",
            |k| hole_widened::build(k).map_err(crate::Refusal::from),
            Some((8368.4956797928, 3563.982297150257)),
        );
    }
    pub mod holes_widened_until_merged;
    #[test]
    fn holes_widened_until_merged() {
        crate::check(
            "cases::edit::holes_widened_until_merged",
            "edit/holes-widened-until-merged",
            |k| holes_widened_until_merged::build(k).map_err(crate::Refusal::from),
            Some((7862.467521335166, 3576.3510454792586)),
        );
    }
    pub mod l_block_riser_moved;
    #[test]
    fn l_block_riser_moved() {
        crate::check(
            "cases::edit::l_block_riser_moved",
            "edit/l-block-riser-moved",
            |k| l_block_riser_moved::build(k).map_err(crate::Refusal::from),
            Some((18000.0, 4600.0)),
        );
    }
    pub mod rounded_block_top_moved_up;
    #[test]
    fn rounded_block_top_moved_up() {
        crate::check(
            "cases::edit::rounded_block_top_moved_up",
            "edit/rounded-block-top-moved-up",
            |k| rounded_block_top_moved_up::build(k).map_err(crate::Refusal::from),
            Some((19122.743338823082, 4584.632726156541)),
        );
    }
}
pub mod feature {
    pub mod hole_through_the_upper_arm;
    #[test]
    fn hole_through_the_upper_arm() {
        crate::check(
            "cases::feature::hole_through_the_upper_arm",
            "feature/hole-through-the-upper-arm",
            |k| hole_through_the_upper_arm::build(k).map_err(crate::Refusal::from),
            Some((11458.407105255379, 5386.2654824574365)),
        );
    }
    pub mod pad_down_to_cylinder;
    #[test]
    fn pad_down_to_cylinder() {
        crate::check(
            "cases::feature::pad_down_to_cylinder",
            "feature/pad-down-to-cylinder",
            |k| pad_down_to_cylinder::build(k).map_err(crate::Refusal::from),
            Some((12751.845965694885, 3268.381948049816)),
        );
    }
    pub mod post_up_to_dome;
    #[test]
    fn post_up_to_dome() {
        crate::check(
            "cases::feature::post_up_to_dome",
            "feature/post-up-to-dome",
            |k| post_up_to_dome::build(k).map_err(crate::Refusal::from),
            Some((7559.30988769618, 7352.665293339964)),
        );
    }
    pub mod rib_revolved_to_a_stop;
    #[test]
    fn rib_revolved_to_a_stop() {
        crate::check(
            "cases::feature::rib_revolved_to_a_stop",
            "feature/rib-revolved-to-a-stop",
            |k| rib_revolved_to_a_stop::build(k).map_err(crate::Refusal::from),
            Some((5085.453107998477, 2445.4459830140295)),
        );
    }
}
pub mod fill {
    pub mod coons_patch_of_four_lines;
    #[test]
    fn coons_patch_of_four_lines() {
        crate::check(
            "cases::fill::coons_patch_of_four_lines",
            "fill/coons-patch-of-four-lines",
            |k| coons_patch_of_four_lines::build(k).map_err(crate::Refusal::from),
            Some((750.0, 528.0789275273404)),
        );
    }
    pub mod coons_patch_of_four_parabolas;
    #[test]
    fn coons_patch_of_four_parabolas() {
        crate::check(
            "cases::fill::coons_patch_of_four_parabolas",
            "fill/coons-patch-of-four-parabolas",
            |k| coons_patch_of_four_parabolas::build(k).map_err(crate::Refusal::from),
            None,
        );
    }
}
pub mod fillet {
    pub mod ball_and_post;
    #[test]
    fn ball_and_post() {
        crate::check(
            "cases::fillet::ball_and_post",
            "fillet/ball-and-post",
            |k| ball_and_post::build(k).map_err(crate::Refusal::from),
            Some((4467.1367089692585, 1395.719075264613)),
        );
    }
    pub mod bezier_dome_one_edge;
    #[test]
    fn bezier_dome_one_edge() {
        crate::check(
            "cases::fillet::bezier_dome_one_edge",
            "fillet/bezier-dome-one-edge",
            |k| bezier_dome_one_edge::build(k).map_err(crate::Refusal::from),
            None,
        );
    }
    pub mod boss_s_curve;
    #[test]
    fn boss_s_curve() {
        crate::check(
            "cases::fillet::boss_s_curve",
            "fillet/boss-s-curve",
            |k| boss_s_curve::build(k).map_err(crate::Refusal::from),
            Some((6718.978234245993, 3911.7283818781953)),
        );
    }
    pub mod box_all_edges;
    #[test]
    fn box_all_edges() {
        crate::check(
            "cases::fillet::box_all_edges",
            "fillet/box-all-edges",
            |k| box_all_edges::build(k).map_err(crate::Refusal::from),
            Some((975.5870138909414, 547.3628179866693)),
        );
    }
    pub mod box_one_edge;
    #[test]
    fn box_one_edge() {
        crate::check(
            "cases::fillet::box_one_edge",
            "fillet/box-one-edge",
            |k| box_one_edge::build(k).map_err(crate::Refusal::from),
            Some((991.4159265358979, 589.6991118430775)),
        );
    }
    pub mod conic_edge;
    #[test]
    fn conic_edge() {
        crate::check(
            "cases::fillet::conic_edge",
            "fillet/conic-edge",
            |k| conic_edge::build(k).map_err(crate::Refusal::from),
            Some((1985.0, 985.696757204207)),
        );
    }
    pub mod cylinder_rim;
    #[test]
    fn cylinder_rim() {
        crate::check(
            "cases::fillet::cylinder_rim",
            "fillet/cylinder-rim",
            |k| cylinder_rim::build(k).map_err(crate::Refusal::from),
            Some((778.957433419712, 457.31024053179993)),
        );
    }
    pub mod cylinder_rim_to_dome;
    #[test]
    fn cylinder_rim_to_dome() {
        crate::check(
            "cases::fillet::cylinder_rim_to_dome",
            "fillet/cylinder-rim-to-dome",
            |k| cylinder_rim_to_dome::build(k).map_err(crate::Refusal::from),
            Some((654.4984694978735, 392.6990816987241)),
        );
    }
    pub mod hole_waist;
    #[test]
    fn hole_waist() {
        crate::check(
            "cases::fillet::hole_waist",
            "fillet/hole-waist",
            |k| hole_waist::build(k).map_err(crate::Refusal::from),
            Some((734.7586624846977, 925.8594996794824)),
        );
    }
    pub mod l_block_end_mixed_corner;
    #[test]
    fn l_block_end_mixed_corner() {
        crate::check(
            "cases::fillet::l_block_end_mixed_corner",
            "fillet/l-block-end-mixed-corner",
            |k| l_block_end_mixed_corner::build(k).map_err(crate::Refusal::from),
            Some((2024.683159826111, 1176.562210449103)),
        );
    }
    pub mod l_block_inside_consumed;
    #[test]
    fn l_block_inside_consumed() {
        crate::check(
            "cases::fillet::l_block_inside_consumed",
            "fillet/l-block-inside-consumed",
            |k| l_block_inside_consumed::build(k).map_err(crate::Refusal::from),
            Some((615.1548999352503, 492.98672286269283)),
        );
    }
    pub mod notched_cube_inside_corner;
    #[test]
    fn notched_cube_inside_corner() {
        crate::check(
            "cases::fillet::notched_cube_inside_corner",
            "fillet/notched-cube-inside-corner",
            |k| notched_cube_inside_corner::build(k).map_err(crate::Refusal::from),
            Some((7053.422580176728, 2365.8915851175016)),
        );
    }
    pub mod nurbs_cylinder_rim;
    #[test]
    fn nurbs_cylinder_rim() {
        crate::check(
            "cases::fillet::nurbs_cylinder_rim",
            "fillet/nurbs-cylinder-rim",
            |k| nurbs_cylinder_rim::build(k).map_err(crate::Refusal::from),
            Some((778.957433419712, 457.31024053179993)),
        );
    }
    pub mod octahedron_all_edges;
    #[test]
    fn octahedron_all_edges() {
        crate::check(
            "cases::fillet::octahedron_all_edges",
            "fillet/octahedron-all-edges",
            |k| octahedron_all_edges::build(k).map_err(crate::Refusal::from),
            Some((1270.7657089174302, 619.2950465096911)),
        );
    }
    pub mod rectangular_pyramid_all_edges;
    #[test]
    fn rectangular_pyramid_all_edges() {
        crate::check(
            "cases::fillet::rectangular_pyramid_all_edges",
            "fillet/rectangular-pyramid-all-edges",
            |k| rectangular_pyramid_all_edges::build(k).map_err(crate::Refusal::from),
            Some((848.6909721602239, 527.6607266664573)),
        );
    }
    pub mod rounded_block_top_rim;
    #[test]
    fn rounded_block_top_rim() {
        crate::check(
            "cases::fillet::rounded_block_top_rim",
            "fillet/rounded-block-top-rim",
            |k| rounded_block_top_rim::build(k).map_err(crate::Refusal::from),
            Some((1149.043632182972, 733.0931753639765)),
        );
    }
    pub mod slab_half_round;
    #[test]
    fn slab_half_round() {
        crate::check(
            "cases::fillet::slab_half_round",
            "fillet/slab-half-round",
            |k| slab_half_round::build(k).map_err(crate::Refusal::from),
            Some((222.83185307179588, 227.39822368615503)),
        );
    }
    pub mod sliver_strip;
    #[test]
    fn sliver_strip() {
        crate::check(
            "cases::fillet::sliver_strip",
            "fillet/sliver-strip",
            |k| sliver_strip::build(k).map_err(crate::Refusal::from),
            Some((391.41764326475004, 501.97533421296373)),
        );
    }
    pub mod square_boss_root_mixed_corners;
    #[test]
    fn square_boss_root_mixed_corners() {
        crate::check(
            "cases::fillet::square_boss_root_mixed_corners",
            "fillet/square-boss-root-mixed-corners",
            |k| square_boss_root_mixed_corners::build(k).map_err(crate::Refusal::from),
            Some((8535.669714402615, 3639.7579738406407)),
        );
    }
    pub mod tee_of_two_cylinders;
    #[test]
    fn tee_of_two_cylinders() {
        crate::check(
            "cases::fillet::tee_of_two_cylinders",
            "fillet/tee-of-two-cylinders",
            |k| tee_of_two_cylinders::build(k).map_err(crate::Refusal::from),
            None,
        );
    }
    pub mod variable_radius_edge;
    #[test]
    fn variable_radius_edge() {
        crate::check(
            "cases::fillet::variable_radius_edge",
            "fillet/variable-radius-edge",
            |k| variable_radius_edge::build(k).map_err(crate::Refusal::from),
            Some((994.9926238126071, 592.4995517137655)),
        );
    }
}
pub mod geometry {
    pub mod ellipsoid;
    #[test]
    fn ellipsoid() {
        crate::check(
            "cases::geometry::ellipsoid",
            "geometry/ellipsoid",
            |k| ellipsoid::build(k).map_err(crate::Refusal::from),
            Some((301.59289474462014, 231.24821501597344)),
        );
    }
    pub mod ellipsoid_half;
    #[test]
    fn ellipsoid_half() {
        crate::check(
            "cases::geometry::ellipsoid_half",
            "geometry/ellipsoid-half",
            |k| ellipsoid_half::build(k).map_err(crate::Refusal::from),
            Some((150.79644737231007, 191.02233119414177)),
        );
    }
    pub mod elliptic_cylinder;
    #[test]
    fn elliptic_cylinder() {
        crate::check(
            "cases::geometry::elliptic_cylinder",
            "geometry/elliptic-cylinder",
            |k| elliptic_cylinder::build(k).map_err(crate::Refusal::from),
            Some((471.23889803846896, 349.51776824167507)),
        );
    }
    pub mod elliptic_hole;
    #[test]
    fn elliptic_hole() {
        crate::check(
            "cases::geometry::elliptic_hole",
            "geometry/elliptic-hole",
            |k| elliptic_hole::build(k).map_err(crate::Refusal::from),
            Some((2619.61065788307, 1527.4254217639116)),
        );
    }
    pub mod high_degree_bezier_dome;
    #[test]
    fn high_degree_bezier_dome() {
        crate::check(
            "cases::geometry::high_degree_bezier_dome",
            "geometry/high-degree-bezier-dome",
            |k| high_degree_bezier_dome::build(k).map_err(crate::Refusal::from),
            Some((2795.3125, 1358.7504625329134)),
        );
    }
    pub mod horn_torus;
    #[test]
    fn horn_torus() {
        crate::check(
            "cases::geometry::horn_torus",
            "geometry/horn-torus",
            |k| horn_torus::build(k).map_err(crate::Refusal::from),
            Some((2467.4011002723396, 986.9604401089358)),
        );
    }
    pub mod hyperboloid;
    #[test]
    fn hyperboloid() {
        crate::check(
            "cases::geometry::hyperboloid",
            "geometry/hyperboloid",
            |k| hyperboloid::build(k).map_err(crate::Refusal::from),
            Some((804.247719318987, 564.4508882371365)),
        );
    }
    pub mod hyperboloid_of_two_sheets;
    #[test]
    fn hyperboloid_of_two_sheets() {
        crate::check(
            "cases::geometry::hyperboloid_of_two_sheets",
            "geometry/hyperboloid-of-two-sheets",
            |k| hyperboloid_of_two_sheets::build(k).map_err(crate::Refusal::from),
            Some((201.06192982974676, 322.7472450737855)),
        );
    }
    pub mod hyperboloid_ruled;
    #[test]
    fn hyperboloid_ruled() {
        crate::check(
            "cases::geometry::hyperboloid_ruled",
            "geometry/hyperboloid-ruled",
            |k| hyperboloid_ruled::build(k).map_err(crate::Refusal::from),
            Some((523.5987755982989, 426.26652204800996)),
        );
    }
    pub mod lemon;
    #[test]
    fn lemon() {
        crate::check(
            "cases::geometry::lemon",
            "geometry/lemon",
            |k| lemon::build(k).map_err(crate::Refusal::from),
            Some((57.299667477372516, 76.53638161221481)),
        );
    }
    pub mod nurbs_cone;
    #[test]
    fn nurbs_cone() {
        crate::check(
            "cases::geometry::nurbs_cone",
            "geometry/nurbs-cone",
            |k| nurbs_cone::build(k).map_err(crate::Refusal::from),
            Some((261.79938779914943, 254.160184615763)),
        );
    }
    pub mod nurbs_sphere;
    #[test]
    fn nurbs_sphere() {
        crate::check(
            "cases::geometry::nurbs_sphere",
            "geometry/nurbs-sphere",
            |k| nurbs_sphere::build(k).map_err(crate::Refusal::from),
            Some((523.5987755982989, 314.1592653589793)),
        );
    }
    pub mod oblate_spheroid;
    #[test]
    fn oblate_spheroid() {
        crate::check(
            "cases::geometry::oblate_spheroid",
            "geometry/oblate-spheroid",
            |k| oblate_spheroid::build(k).map_err(crate::Refusal::from),
            Some((452.3893421169302, 312.18777732042184)),
        );
    }
    pub mod paraboloid;
    #[test]
    fn paraboloid() {
        crate::check(
            "cases::geometry::paraboloid",
            "geometry/paraboloid",
            |k| paraboloid::build(k).map_err(crate::Refusal::from),
            Some((804.247719318987, 542.208393846961)),
        );
    }
    pub mod periodic_bspline_prism;
    #[test]
    fn periodic_bspline_prism() {
        crate::check(
            "cases::geometry::periodic_bspline_prism",
            "geometry/periodic-bspline-prism",
            |k| periodic_bspline_prism::build(k).map_err(crate::Refusal::from),
            Some((3333.3333333333335, 1315.9567627227589)),
        );
    }
    pub mod periodic_bspline_ring;
    #[test]
    fn periodic_bspline_ring() {
        crate::check(
            "cases::geometry::periodic_bspline_ring",
            "geometry/periodic-bspline-ring",
            |k| periodic_bspline_ring::build(k).map_err(crate::Refusal::from),
            Some((1884.9555921538758, 1223.8829974910582)),
        );
    }
    pub mod saddle_drum;
    #[test]
    fn saddle_drum() {
        crate::check(
            "cases::geometry::saddle_drum",
            "geometry/saddle-drum",
            |k| saddle_drum::build(k).map_err(crate::Refusal::from),
            Some((2010.6192982974676, 948.8014785737964)),
        );
    }
    pub mod saddle_drum_bored;
    #[test]
    fn saddle_drum_bored() {
        crate::check(
            "cases::geometry::saddle_drum_bored",
            "geometry/saddle-drum-bored",
            |k| saddle_drum_bored::build(k).map_err(crate::Refusal::from),
            Some((1727.8759594743863, 1079.7764991189633)),
        );
    }
    pub mod spindle_torus;
    #[test]
    fn spindle_torus() {
        crate::check(
            "cases::geometry::spindle_torus",
            "geometry/spindle-torus",
            |k| spindle_torus::build(k).map_err(crate::Refusal::from),
            Some((1537.7403276407763, 668.7126456775763)),
        );
    }
    pub mod torus_nearly_horn;
    #[test]
    fn torus_nearly_horn() {
        crate::check(
            "cases::geometry::torus_nearly_horn",
            "geometry/torus-nearly-horn",
            |k| torus_nearly_horn::build(k).map_err(crate::Refusal::from),
            Some((2457.541365475651, 984.986519228718)),
        );
    }
}
pub mod integration {
    pub mod utah_teapot;
    #[test]
    fn utah_teapot() {
        crate::check(
            "cases::integration::utah_teapot",
            "integration/utah-teapot",
            |k| utah_teapot::build(k).map_err(crate::Refusal::from),
            None,
        );
    }
}
pub mod loft {
    pub mod slab_pinched_on_four_guides;
    #[test]
    fn slab_pinched_on_four_guides() {
        crate::check(
            "cases::loft::slab_pinched_on_four_guides",
            "loft/slab-pinched-on-four-guides",
            |k| slab_pinched_on_four_guides::build(k).map_err(crate::Refusal::from),
            Some((2436.0685714285714, 2124.0)),
        );
    }
    pub mod square_spire_on_four_guides;
    #[test]
    fn square_spire_on_four_guides() {
        crate::check(
            "cases::loft::square_spire_on_four_guides",
            "loft/square-spire-on-four-guides",
            |k| square_spire_on_four_guides::build(k).map_err(crate::Refusal::from),
            Some((9423.771428571428, 2397.6)),
        );
    }
    pub mod square_to_octagon;
    #[test]
    fn square_to_octagon() {
        crate::check(
            "cases::loft::square_to_octagon",
            "loft/square-to-octagon",
            |k| square_to_octagon::build(k).map_err(crate::Refusal::from),
            Some((33760.0, 6188.254698053646)),
        );
    }
    pub mod square_to_round;
    #[test]
    fn square_to_round() {
        crate::check(
            "cases::loft::square_to_round",
            "loft/square-to-round",
            |k| square_to_round::build(k).map_err(crate::Refusal::from),
            Some((35068.58347057703, 6241.372840065616)),
        );
    }
    pub mod twisted_tapering_square;
    #[test]
    fn twisted_tapering_square() {
        crate::check(
            "cases::loft::twisted_tapering_square",
            "loft/twisted-tapering-square",
            |k| twisted_tapering_square::build(k).map_err(crate::Refusal::from),
            Some((17333.333333333332, 4643.764402962483)),
        );
    }
    pub mod twisted_thin_blade;
    #[test]
    fn twisted_thin_blade() {
        crate::check(
            "cases::loft::twisted_thin_blade",
            "loft/twisted-thin-blade",
            |k| twisted_thin_blade::build(k).map_err(crate::Refusal::from),
            Some((960.0, 7351.884474938799)),
        );
    }
}
pub mod nurbs {
    pub mod bezier_dome;
    #[test]
    fn bezier_dome() {
        crate::check(
            "cases::nurbs::bezier_dome",
            "nurbs/bezier-dome",
            |k| bezier_dome::build(k).map_err(crate::Refusal::from),
            Some((3150.0, 1309.322926203161)),
        );
    }
    pub mod bspline_dome;
    #[test]
    fn bspline_dome() {
        crate::check(
            "cases::nurbs::bspline_dome",
            "nurbs/bspline-dome",
            |k| bspline_dome::build(k).map_err(crate::Refusal::from),
            Some((3081.0055555555555, 1285.368404802979)),
        );
    }
    pub mod drilled_dome;
    #[test]
    fn drilled_dome() {
        crate::check(
            "cases::nurbs::drilled_dome",
            "nurbs/drilled-dome",
            |k| drilled_dome::build(k).map_err(crate::Refusal::from),
            Some((2872.944892550934, 1431.7956857724896)),
        );
    }
    pub mod drum_in_a_nurbs_sleeve;
    #[test]
    fn drum_in_a_nurbs_sleeve() {
        crate::check(
            "cases::nurbs::drum_in_a_nurbs_sleeve",
            "nurbs/drum-in-a-nurbs-sleeve",
            |k| drum_in_a_nurbs_sleeve::build(k).map_err(crate::Refusal::from),
            Some((1237.7875055143786, 697.4335690969341)),
        );
    }
    pub mod flat_bspline_slabs_united;
    #[test]
    fn flat_bspline_slabs_united() {
        crate::check(
            "cases::nurbs::flat_bspline_slabs_united",
            "nurbs/flat-bspline-slabs-united",
            |k| flat_bspline_slabs_united::build(k).map_err(crate::Refusal::from),
            Some((700.0, 610.0)),
        );
    }
    pub mod groin_vault;
    #[test]
    fn groin_vault() {
        crate::check(
            "cases::nurbs::groin_vault",
            "nurbs/groin-vault",
            |k| groin_vault::build(k).map_err(crate::Refusal::from),
            Some((7632.0, 2256.0)),
        );
    }
    pub mod lens_of_two_nurbs_balls;
    #[test]
    fn lens_of_two_nurbs_balls() {
        crate::check(
            "cases::nurbs::lens_of_two_nurbs_balls",
            "nurbs/lens-of-two-nurbs-balls",
            |k| lens_of_two_nurbs_balls::build(k).map_err(crate::Refusal::from),
            Some((108.90854532444617, 125.66370614359172)),
        );
    }
    pub mod sliced_dome;
    #[test]
    fn sliced_dome() {
        crate::check(
            "cases::nurbs::sliced_dome",
            "nurbs/sliced-dome",
            |k| sliced_dome::build(k).map_err(crate::Refusal::from),
            Some((2854.4122952001485, 1276.254114612174)),
        );
    }
    pub mod vault_tangent_plane;
    #[test]
    fn vault_tangent_plane() {
        crate::check(
            "cases::nurbs::vault_tangent_plane",
            "nurbs/vault-tangent-plane",
            |k| vault_tangent_plane::build(k).map_err(crate::Refusal::from),
            Some((8175.394177208706, 2404.9354050618863)),
        );
    }
}
pub mod offset {
    pub mod box_grown;
    #[test]
    fn box_grown() {
        crate::check(
            "cases::offset::box_grown",
            "offset/box-grown",
            |k| box_grown::build(k).map_err(crate::Refusal::from),
            Some((935.5870138909414, 539.3628179866691)),
        );
    }
    pub mod box_shrunk;
    #[test]
    fn box_shrunk() {
        crate::check(
            "cases::offset::box_shrunk",
            "offset/box-shrunk",
            |k| box_shrunk::build(k).map_err(crate::Refusal::from),
            Some((192.0, 208.0)),
        );
    }
    pub mod cone_grown;
    #[test]
    fn cone_grown() {
        crate::check(
            "cases::offset::cone_grown",
            "offset/cone-grown",
            |k| cone_grown::build(k).map_err(crate::Refusal::from),
            Some((150.72175801789774, 154.83585886827183)),
        );
    }
    pub mod cylinder_grown;
    #[test]
    fn cylinder_grown() {
        crate::check(
            "cases::offset::cylinder_grown",
            "offset/cylinder-grown",
            |k| cylinder_grown::build(k).map_err(crate::Refusal::from),
            Some((1341.5898001820483, 645.3331657355175)),
        );
    }
    pub mod drilled_plate_grown;
    #[test]
    fn drilled_plate_grown() {
        crate::check(
            "cases::offset::drilled_plate_grown",
            "offset/drilled-plate-grown",
            |k| drilled_plate_grown::build(k).map_err(crate::Refusal::from),
            Some((2781.0247397391663, 1449.3945946152583)),
        );
    }
    pub mod dumbbell_split;
    #[test]
    fn dumbbell_split() {
        crate::check(
            "cases::offset::dumbbell_split",
            "offset/dumbbell-split",
            |k| dumbbell_split::build(k).map_err(crate::Refusal::from),
            Some((295.4404727084366, 366.09541295104316)),
        );
    }
    pub mod elliptic_cylinder_grown;
    #[test]
    fn elliptic_cylinder_grown() {
        crate::check(
            "cases::offset::elliptic_cylinder_grown",
            "offset/elliptic-cylinder-grown",
            |k| elliptic_cylinder_grown::build(k).map_err(crate::Refusal::from),
            Some((896.4590990695517, 505.1114240252767)),
        );
    }
    pub mod elliptic_cylinder_shrunk;
    #[test]
    fn elliptic_cylinder_shrunk() {
        crate::check(
            "cases::offset::elliptic_cylinder_shrunk",
            "offset/elliptic-cylinder-shrunk",
            |k| elliptic_cylinder_shrunk::build(k).map_err(crate::Refusal::from),
            Some((197.9078687523085, 203.42747563782547)),
        );
    }
    pub mod elliptic_cylinder_shrunk_past_its_curvature;
    #[test]
    fn elliptic_cylinder_shrunk_past_its_curvature() {
        crate::check(
            "cases::offset::elliptic_cylinder_shrunk_past_its_curvature",
            "offset/elliptic-cylinder-shrunk-past-its-curvature",
            |k| elliptic_cylinder_shrunk_past_its_curvature::build(k).map_err(crate::Refusal::from),
            None,
        );
    }
    pub mod flanged_drum_shrunk;
    #[test]
    fn flanged_drum_shrunk() {
        crate::check(
            "cases::offset::flanged_drum_shrunk",
            "offset/flanged-drum-shrunk",
            |k| flanged_drum_shrunk::build(k).map_err(crate::Refusal::from),
            Some((624.3662286501782, 496.6023797475794)),
        );
    }
    pub mod grooved_drum_grown;
    #[test]
    fn grooved_drum_grown() {
        crate::check(
            "cases::offset::grooved_drum_grown",
            "offset/grooved-drum-grown",
            |k| grooved_drum_grown::build(k).map_err(crate::Refusal::from),
            Some((4519.350146936107, 1545.4237620122985)),
        );
    }
    pub mod l_block_grown;
    #[test]
    fn l_block_grown() {
        crate::check(
            "cases::offset::l_block_grown",
            "offset/l-block-grown",
            |k| l_block_grown::build(k).map_err(crate::Refusal::from),
            Some((2197.27432383419, 1138.4513020910304)),
        );
    }
    pub mod l_block_grown_sharp;
    #[test]
    fn l_block_grown_sharp() {
        crate::check(
            "cases::offset::l_block_grown_sharp",
            "offset/l-block-grown-sharp",
            |k| l_block_grown_sharp::build(k).map_err(crate::Refusal::from),
            Some((2240.0, 1228.0)),
        );
    }
    pub mod l_block_shrunk;
    #[test]
    fn l_block_shrunk() {
        crate::check(
            "cases::offset::l_block_shrunk",
            "offset/l-block-shrunk",
            |k| l_block_shrunk::build(k).map_err(crate::Refusal::from),
            Some((505.2876110196153, 537.8539816339745)),
        );
    }
    pub mod napkin_ring_grown;
    #[test]
    fn napkin_ring_grown() {
        crate::check(
            "cases::offset::napkin_ring_grown",
            "offset/napkin-ring-grown",
            |k| napkin_ring_grown::build(k).map_err(crate::Refusal::from),
            Some((737.0776973335801, 535.8663887950087)),
        );
    }
    pub mod nurbs_ball_grown;
    #[test]
    fn nurbs_ball_grown() {
        crate::check(
            "cases::offset::nurbs_ball_grown",
            "offset/nurbs-ball-grown",
            |k| nurbs_ball_grown::build(k).map_err(crate::Refusal::from),
            Some((1436.755040241732, 615.7521601035994)),
        );
    }
    pub mod tetrahedron_grown;
    #[test]
    fn tetrahedron_grown() {
        crate::check(
            "cases::offset::tetrahedron_grown",
            "offset/tetrahedron-grown",
            |k| tetrahedron_grown::build(k).map_err(crate::Refusal::from),
            Some((249.53323018738044, 234.54759243458807)),
        );
    }
    pub mod torus_grown_past_its_hole;
    #[test]
    fn torus_grown_past_its_hole() {
        crate::check(
            "cases::offset::torus_grown_past_its_hole",
            "offset/torus-grown-past-its-hole",
            |k| torus_grown_past_its_hole::build(k).map_err(crate::Refusal::from),
            Some((1984.2874392993506, 816.3344428255867)),
        );
    }
    pub mod torus_shrunk;
    #[test]
    fn torus_shrunk() {
        crate::check(
            "cases::offset::torus_shrunk",
            "offset/torus-shrunk",
            |k| torus_shrunk::build(k).map_err(crate::Refusal::from),
            Some((444.13219804902116, 592.1762640653615)),
        );
    }
}
pub mod parts {
    pub mod flange;
    #[test]
    fn flange() {
        crate::check(
            "cases::parts::flange",
            "parts/flange",
            |k| flange::build(k).map_err(crate::Refusal::from),
            Some((22135.66183719368, 9324.246995854506)),
        );
    }
    pub mod pin_lug;
    #[test]
    fn pin_lug() {
        crate::check(
            "cases::parts::pin_lug",
            "parts/pin-lug",
            |k| pin_lug::build(k).map_err(crate::Refusal::from),
            Some((29989.380701702525, 8900.530964914875)),
        );
    }
    pub mod pocketed_tray;
    #[test]
    fn pocketed_tray() {
        crate::check(
            "cases::parts::pocketed_tray",
            "parts/pocketed-tray",
            |k| pocketed_tray::build(k).map_err(crate::Refusal::from),
            Some((21371.150263212312, 14967.43350880821)),
        );
    }
    pub mod shelf_bracket;
    #[test]
    fn shelf_bracket() {
        crate::check(
            "cases::parts::shelf_bracket",
            "parts/shelf-bracket",
            |k| shelf_bracket::build(k).map_err(crate::Refusal::from),
            Some((40137.3451754256, 12393.13274122872)),
        );
    }
    pub mod star_handle;
    #[test]
    fn star_handle() {
        crate::check(
            "cases::parts::star_handle",
            "parts/star-handle",
            |k| star_handle::build(k).map_err(crate::Refusal::from),
            Some((19179.040932717548, 6148.760045500778)),
        );
    }
    pub mod turned_shaft;
    #[test]
    fn turned_shaft() {
        crate::check(
            "cases::parts::turned_shaft",
            "parts/turned-shaft",
            |k| turned_shaft::build(k).map_err(crate::Refusal::from),
            Some((120915.32332987145, 17470.591304910977)),
        );
    }
    pub mod v_belt_pulley;
    #[test]
    fn v_belt_pulley() {
        crate::check(
            "cases::parts::v_belt_pulley",
            "parts/v-belt-pulley",
            |k| v_belt_pulley::build(k).map_err(crate::Refusal::from),
            Some((41490.56537700988, 10484.569630424683)),
        );
    }
}
pub mod primitives {
    pub mod r#box;
    #[test]
    fn r#box() {
        crate::check(
            "cases::primitives::r#box",
            "primitives/box",
            |k| r#box::build(k).map_err(crate::Refusal::from),
            Some((6000.0, 2200.0)),
        );
    }
    pub mod cone_frustum;
    #[test]
    fn cone_frustum() {
        crate::check(
            "cases::primitives::cone_frustum",
            "primitives/cone-frustum",
            |k| cone_frustum::build(k).map_err(crate::Refusal::from),
            Some((245.04422698000386, 238.62729630595925)),
        );
    }
    pub mod cylinder;
    #[test]
    fn cylinder() {
        crate::check(
            "cases::primitives::cylinder",
            "primitives/cylinder",
            |k| cylinder::build(k).map_err(crate::Refusal::from),
            Some((785.3981633974482, 471.23889803846896)),
        );
    }
    pub mod extruded_l;
    #[test]
    fn extruded_l() {
        crate::check(
            "cases::primitives::extruded_l",
            "primitives/extruded-l",
            |k| extruded_l::build(k).map_err(crate::Refusal::from),
            Some((1200.0, 860.0)),
        );
    }
    pub mod revolved_sector;
    #[test]
    fn revolved_sector() {
        crate::check(
            "cases::primitives::revolved_sector",
            "primitives/revolved-sector",
            |k| revolved_sector::build(k).map_err(crate::Refusal::from),
            Some((201.06192982974676, 233.06192982974676)),
        );
    }
    pub mod revolved_washer;
    #[test]
    fn revolved_washer() {
        crate::check(
            "cases::primitives::revolved_washer",
            "primitives/revolved-washer",
            |k| revolved_washer::build(k).map_err(crate::Refusal::from),
            Some((804.247719318987, 804.247719318987)),
        );
    }
    pub mod sphere;
    #[test]
    fn sphere() {
        crate::check(
            "cases::primitives::sphere",
            "primitives/sphere",
            |k| sphere::build(k).map_err(crate::Refusal::from),
            Some((523.5987755982989, 314.1592653589793)),
        );
    }
    pub mod torus;
    #[test]
    fn torus() {
        crate::check(
            "cases::primitives::torus",
            "primitives/torus",
            |k| torus::build(k).map_err(crate::Refusal::from),
            Some((789.5683520871487, 789.5683520871487)),
        );
    }
}
pub mod shell {
    pub mod box_five_faces_open;
    #[test]
    fn box_five_faces_open() {
        crate::check(
            "cases::shell::box_five_faces_open",
            "shell/box-five-faces-open",
            |k| box_five_faces_open::build(k).map_err(crate::Refusal::from),
            Some((80.0, 196.0)),
        );
    }
    pub mod box_hairline_cavity;
    #[test]
    fn box_hairline_cavity() {
        crate::check(
            "cases::shell::box_hairline_cavity",
            "shell/box-hairline-cavity",
            |k| box_hairline_cavity::build(k).map_err(crate::Refusal::from),
            Some((479.918796, 384.2008)),
        );
    }
    pub mod box_hollowed;
    #[test]
    fn box_hollowed() {
        crate::check(
            "cases::shell::box_hollowed",
            "shell/box-hollowed",
            |k| box_hollowed::build(k).map_err(crate::Refusal::from),
            Some((288.0, 584.0)),
        );
    }
    pub mod box_open_top;
    #[test]
    fn box_open_top() {
        crate::check(
            "cases::shell::box_open_top",
            "shell/box-open-top",
            |k| box_open_top::build(k).map_err(crate::Refusal::from),
            Some((240.0, 516.0)),
        );
    }
    pub mod box_open_top_and_bottom;
    #[test]
    fn box_open_top_and_bottom() {
        crate::check(
            "cases::shell::box_open_top_and_bottom",
            "shell/box-open-top-and-bottom",
            |k| box_open_top_and_bottom::build(k).map_err(crate::Refusal::from),
            Some((192.0, 448.0)),
        );
    }
    pub mod box_open_top_and_front;
    #[test]
    fn box_open_top_and_front() {
        crate::check(
            "cases::shell::box_open_top_and_front",
            "shell/box-open-top-and-front",
            |k| box_open_top_and_front::build(k).map_err(crate::Refusal::from),
            Some((200.0, 446.0)),
        );
    }
    pub mod box_open_top_outward;
    #[test]
    fn box_open_top_outward() {
        crate::check(
            "cases::shell::box_open_top_outward",
            "shell/box-open-top-outward",
            |k| box_open_top_outward::build(k).map_err(crate::Refusal::from),
            Some((345.2182849062401, 731.672557568463)),
        );
    }
    pub mod box_open_top_outward_sharp;
    #[test]
    fn box_open_top_outward_sharp() {
        crate::check(
            "cases::shell::box_open_top_outward_sharp",
            "shell/box-open-top-outward-sharp",
            |k| box_open_top_outward_sharp::build(k).map_err(crate::Refusal::from),
            Some((360.0, 764.0)),
        );
    }
    pub mod box_paper_thin;
    #[test]
    fn box_paper_thin() {
        crate::check(
            "cases::shell::box_paper_thin",
            "shell/box-paper-thin",
            |k| box_paper_thin::build(k).map_err(crate::Refusal::from),
            Some((0.29594000399998777, 591.9160079999999)),
        );
    }
    pub mod box_walls_meeting;
    #[test]
    fn box_walls_meeting() {
        crate::check(
            "cases::shell::box_walls_meeting",
            "shell/box-walls-meeting",
            |k| box_walls_meeting::build(k).map_err(crate::Refusal::from),
            Some((480.0, 376.0)),
        );
    }
    pub mod capsule_open_at_its_dome;
    #[test]
    fn capsule_open_at_its_dome() {
        crate::check(
            "cases::shell::capsule_open_at_its_dome",
            "shell/capsule-open-at-its-dome",
            |k| capsule_open_at_its_dome::build(k).map_err(crate::Refusal::from),
            Some((389.55748904513433, 838.8052385084748)),
        );
    }
    pub mod capsule_open_base;
    #[test]
    fn capsule_open_base() {
        crate::check(
            "cases::shell::capsule_open_base",
            "shell/capsule-open-base",
            |k| capsule_open_base::build(k).map_err(crate::Refusal::from),
            Some((410.5014400690663, 851.3716091228339)),
        );
    }
    pub mod cone_open_base;
    #[test]
    fn cone_open_base() {
        crate::check(
            "cases::shell::cone_open_base",
            "shell/cone-open-base",
            |k| cone_open_base::build(k).map_err(crate::Refusal::from),
            Some((151.9527280017563, 348.8476842423666)),
        );
    }
    pub mod cylinder_open_top;
    #[test]
    fn cylinder_open_top() {
        crate::check(
            "cases::shell::cylinder_open_top",
            "shell/cylinder-open-top",
            |k| cylinder_open_top::build(k).map_err(crate::Refusal::from),
            Some((333.0088212805181, 697.4335690969341)),
        );
    }
    pub mod cylinder_side_open;
    #[test]
    fn cylinder_side_open() {
        crate::check(
            "cases::shell::cylinder_side_open",
            "shell/cylinder-side-open",
            |k| cylinder_side_open::build(k).map_err(crate::Refusal::from),
            Some((157.07963267948966, 376.99111843077515)),
        );
    }
    pub mod drilled_block_open_at_its_bore;
    #[test]
    fn drilled_block_open_at_its_bore() {
        crate::check(
            "cases::shell::drilled_block_open_at_its_bore",
            "shell/drilled-block-open-at-its-bore",
            |k| drilled_block_open_at_its_bore::build(k).map_err(crate::Refusal::from),
            Some((1351.4513322353837, 2748.6017763138448)),
        );
    }
    pub mod drilled_block_open_top;
    #[test]
    fn drilled_block_open_top() {
        crate::check(
            "cases::shell::drilled_block_open_top",
            "shell/drilled-block-open-top",
            |k| drilled_block_open_top::build(k).map_err(crate::Refusal::from),
            Some((1253.646003293849, 2606.1415625092363)),
        );
    }
    pub mod dumbbell_neck_filled;
    #[test]
    fn dumbbell_neck_filled() {
        crate::check(
            "cases::shell::dumbbell_neck_filled",
            "shell/dumbbell-neck-filled",
            |k| dumbbell_neck_filled::build(k).map_err(crate::Refusal::from),
            Some((804.8392909373451, 1173.702646718128)),
        );
    }
    pub mod half_torus_open_ends;
    #[test]
    fn half_torus_open_ends() {
        crate::check(
            "cases::shell::half_torus_open_ends",
            "shell/half-torus-open-ends",
            |k| half_torus_open_ends::build(k).map_err(crate::Refusal::from),
            Some((103.63084621143825, 425.5189591333173)),
        );
    }
    pub mod hemisphere_open;
    #[test]
    fn hemisphere_open() {
        crate::check(
            "cases::shell::hemisphere_open",
            "shell/hemisphere-open",
            |k| hemisphere_open::build(k).map_err(crate::Refusal::from),
            Some((127.75810124598492, 285.88493147667117)),
        );
    }
    pub mod hole_skin_merging_wall;
    #[test]
    fn hole_skin_merging_wall() {
        crate::check(
            "cases::shell::hole_skin_merging_wall",
            "shell/hole-skin-merging-wall",
            |k| hole_skin_merging_wall::build(k).map_err(crate::Refusal::from),
            Some((1241.873577515697, 2534.8987556529105)),
        );
    }
    pub mod l_block_open_top;
    #[test]
    fn l_block_open_top() {
        crate::check(
            "cases::shell::l_block_open_top",
            "shell/l-block-open-top",
            |k| l_block_open_top::build(k).map_err(crate::Refusal::from),
            Some((610.4977871437821, 1290.9955742875643)),
        );
    }
    pub mod l_block_open_top_sharp;
    #[test]
    fn l_block_open_top_sharp() {
        crate::check(
            "cases::shell::l_block_open_top_sharp",
            "shell/l-block-open-top-sharp",
            |k| l_block_open_top_sharp::build(k).map_err(crate::Refusal::from),
            Some((612.0, 1294.0)),
        );
    }
    pub mod round_consumed;
    #[test]
    fn round_consumed() {
        crate::check(
            "cases::shell::round_consumed",
            "shell/round-consumed",
            |k| round_consumed::build(k).map_err(crate::Refusal::from),
            Some((1981.6637061435918, 1506.79644737231)),
        );
    }
    pub mod round_shrunk_to_an_edge;
    #[test]
    fn round_shrunk_to_an_edge() {
        crate::check(
            "cases::shell::round_shrunk_to_an_edge",
            "shell/round-shrunk-to-an-edge",
            |k| round_shrunk_to_an_edge::build(k).map_err(crate::Refusal::from),
            Some((1485.6637061435918, 1614.79644737231)),
        );
    }
    pub mod rounded_tray;
    #[test]
    fn rounded_tray() {
        crate::check(
            "cases::shell::rounded_tray",
            "shell/rounded-tray",
            |k| rounded_tray::build(k).map_err(crate::Refusal::from),
            Some((829.3893722612838, 1723.3451151369263)),
        );
    }
    pub mod sphere_hollowed;
    #[test]
    fn sphere_hollowed() {
        crate::check(
            "cases::shell::sphere_hollowed",
            "shell/sphere-hollowed",
            |k| sphere_hollowed::build(k).map_err(crate::Refusal::from),
            Some((255.51620249196984, 515.221195188726)),
        );
    }
    pub mod sphere_open_at_a_cap;
    #[test]
    fn sphere_open_at_a_cap() {
        crate::check(
            "cases::shell::sphere_open_at_a_cap",
            "shell/sphere-open-at-a-cap",
            |k| sphere_open_at_a_cap::build(k).map_err(crate::Refusal::from),
            Some((212.58110289290934, 455.53093477052)),
        );
    }
    pub mod sphere_opening_sealed;
    #[test]
    fn sphere_opening_sealed() {
        crate::check(
            "cases::shell::sphere_opening_sealed",
            "shell/sphere-opening-sealed",
            |k| sphere_opening_sealed::build(k).map_err(crate::Refusal::from),
            Some((251.7201113688822, 514.4357970253286)),
        );
    }
    pub mod sphere_pinhole_opening;
    #[test]
    fn sphere_pinhole_opening() {
        crate::check(
            "cases::shell::sphere_pinhole_opening",
            "shell/sphere-pinhole-opening",
            |k| sphere_pinhole_opening::build(k).map_err(crate::Refusal::from),
            Some((240.8526093418292, 512.0739476683598)),
        );
    }
    pub mod torus_hollowed;
    #[test]
    fn torus_hollowed() {
        crate::check(
            "cases::shell::torus_hollowed",
            "shell/torus-hollowed",
            |k| torus_hollowed::build(k).map_err(crate::Refusal::from),
            Some((207.2616924228765, 829.046769691506)),
        );
    }
}
pub mod singular {
    pub mod ball_cut_aslant_through_a_pole;
    #[test]
    fn ball_cut_aslant_through_a_pole() {
        crate::check(
            "cases::singular::ball_cut_aslant_through_a_pole",
            "singular/ball-cut-aslant-through-a-pole",
            |k| ball_cut_aslant_through_a_pole::build(k).map_err(crate::Refusal::from),
            Some((493.19954082823097, 307.4216143033212)),
        );
    }
    pub mod ball_halved_through_its_poles;
    #[test]
    fn ball_halved_through_its_poles() {
        crate::check(
            "cases::singular::ball_halved_through_its_poles",
            "singular/ball-halved-through-its-poles",
            |k| ball_halved_through_its_poles::build(k).map_err(crate::Refusal::from),
            Some((261.79938779914943, 235.61944901923448)),
        );
    }
    pub mod balls_meeting_through_a_pole;
    #[test]
    fn balls_meeting_through_a_pole() {
        crate::check(
            "cases::singular::balls_meeting_through_a_pole",
            "singular/balls-meeting-through-a-pole",
            |k| balls_meeting_through_a_pole::build(k).map_err(crate::Refusal::from),
            Some((938.2890058721515, 502.6548245743669)),
        );
    }
    pub mod cone_cut_aslant_through_its_apex;
    #[test]
    fn cone_cut_aslant_through_its_apex() {
        crate::check(
            "cases::singular::cone_cut_aslant_through_its_apex",
            "singular/cone-cut-aslant-through-its-apex",
            |k| cone_cut_aslant_through_its_apex::build(k).map_err(crate::Refusal::from),
            Some((210.61731702378458, 224.8993683028736)),
        );
    }
    pub mod cone_halved_through_its_apex;
    #[test]
    fn cone_halved_through_its_apex() {
        crate::check(
            "cases::singular::cone_halved_through_its_apex",
            "singular/cone-halved-through-its-apex",
            |k| cone_halved_through_its_apex::build(k).map_err(crate::Refusal::from),
            Some((130.89969389957471, 177.0800923078815)),
        );
    }
    pub mod cylinder_hoof;
    #[test]
    fn cylinder_hoof() {
        crate::check(
            "cases::singular::cylinder_hoof",
            "singular/cylinder-hoof",
            |k| cylinder_hoof::build(k).map_err(crate::Refusal::from),
            Some((48.11252243246881, 113.48240573520914)),
        );
    }
    pub mod napkin_ring_poles_near_its_rims;
    #[test]
    fn napkin_ring_poles_near_its_rims() {
        crate::check(
            "cases::singular::napkin_ring_poles_near_its_rims",
            "singular/napkin-ring-poles-near-its-rims",
            |k| napkin_ring_poles_near_its_rims::build(k).map_err(crate::Refusal::from),
            Some((268.082573106329, 402.1238596594935)),
        );
    }
    pub mod napkin_ring_poles_on_its_rims;
    #[test]
    fn napkin_ring_poles_on_its_rims() {
        crate::check(
            "cases::singular::napkin_ring_poles_on_its_rims",
            "singular/napkin-ring-poles-on-its-rims",
            |k| napkin_ring_poles_on_its_rims::build(k).map_err(crate::Refusal::from),
            Some((268.082573106329, 402.1238596594935)),
        );
    }
    pub mod saddle_drum_cut_level_through_its_saddle;
    #[test]
    fn saddle_drum_cut_level_through_its_saddle() {
        crate::check(
            "cases::singular::saddle_drum_cut_level_through_its_saddle",
            "singular/saddle-drum-cut-level-through-its-saddle",
            |k| saddle_drum_cut_level_through_its_saddle::build(k).map_err(crate::Refusal::from),
            Some((1882.6192982974676, 862.7900814038285)),
        );
    }
    pub mod sectors_sharing_an_apex;
    #[test]
    fn sectors_sharing_an_apex() {
        crate::check(
            "cases::singular::sectors_sharing_an_apex",
            "singular/sectors-sharing-an-apex",
            |k| sectors_sharing_an_apex::build(k).map_err(crate::Refusal::from),
            Some((146.51640605521118, 179.05709358777133)),
        );
    }
}
pub mod spline {
    pub mod peanut_prism;
    #[test]
    fn peanut_prism() {
        crate::check(
            "cases::spline::peanut_prism",
            "spline/peanut-prism",
            |k| peanut_prism::build(k).map_err(crate::Refusal::from),
            Some((2472.152777777778, 1319.2165440112476)),
        );
    }
    pub mod pipe_on_planar_cubic;
    #[test]
    fn pipe_on_planar_cubic() {
        crate::check(
            "cases::spline::pipe_on_planar_cubic",
            "spline/pipe-on-planar-cubic",
            |k| pipe_on_planar_cubic::build(k).map_err(crate::Refusal::from),
            Some((65345.12719466769, 9047.786842338604)),
        );
    }
    pub mod pipe_on_twisted_cubic;
    #[test]
    fn pipe_on_twisted_cubic() {
        crate::check(
            "cases::spline::pipe_on_twisted_cubic",
            "spline/pipe-on-twisted-cubic",
            |k| pipe_on_twisted_cubic::build(k).map_err(crate::Refusal::from),
            Some((8042.47719318987, 2412.743157956961)),
        );
    }
    pub mod rational_cap;
    #[test]
    fn rational_cap() {
        crate::check(
            "cases::spline::rational_cap",
            "spline/rational-cap",
            |k| rational_cap::build(k).map_err(crate::Refusal::from),
            Some((3534.2917352885174, 1178.0972450961724)),
        );
    }
    pub mod rational_spheroid;
    #[test]
    fn rational_spheroid() {
        crate::check(
            "cases::spline::rational_spheroid",
            "spline/rational-spheroid",
            |k| rational_spheroid::build(k).map_err(crate::Refusal::from),
            Some((1507.9644737231006, 663.1722477458867)),
        );
    }
    pub mod revolved_cubic_dome;
    #[test]
    fn revolved_cubic_dome() {
        crate::check(
            "cases::spline::revolved_cubic_dome",
            "spline/revolved-cubic-dome",
            |k| revolved_cubic_dome::build(k).map_err(crate::Refusal::from),
            Some((2464.7140764263468, 983.9468191043231)),
        );
    }
    pub mod square_on_planar_cubic;
    #[test]
    fn square_on_planar_cubic() {
        crate::check(
            "cases::spline::square_on_planar_cubic",
            "spline/square-on-planar-cubic",
            |k| square_on_planar_cubic::build(k).map_err(crate::Refusal::from),
            Some((16000.0, 4000.0)),
        );
    }
}
pub mod sweep {
    pub mod circle_along_arc;
    #[test]
    fn circle_along_arc() {
        crate::check(
            "cases::sweep::circle_along_arc",
            "sweep/circle-along-arc",
            |k| circle_along_arc::build(k).map_err(crate::Refusal::from),
            Some((197.39208802178717, 222.52482925050552)),
        );
    }
    pub mod duct_elbow;
    #[test]
    fn duct_elbow() {
        crate::check(
            "cases::sweep::duct_elbow",
            "sweep/duct-elbow",
            |k| duct_elbow::build(k).map_err(crate::Refusal::from),
            Some((6031.857894892429, 6287.857894892418)),
        );
    }
    pub mod helical_pipe;
    #[test]
    fn helical_pipe() {
        crate::check(
            "cases::sweep::helical_pipe",
            "sweep/helical-pipe",
            |k| helical_pipe::build(k).map_err(crate::Refusal::from),
            Some((594.048305033062, 1194.3797953733035)),
        );
    }
    pub mod helical_pipe_round_ends;
    #[test]
    fn helical_pipe_round_ends() {
        crate::check(
            "cases::sweep::helical_pipe_round_ends",
            "sweep/helical-pipe-round-ends",
            |k| helical_pipe_round_ends::build(k).map_err(crate::Refusal::from),
            Some((598.2370952378484, 1200.662980680483)),
        );
    }
    pub mod hollow_reducer;
    #[test]
    fn hollow_reducer() {
        crate::check(
            "cases::sweep::hollow_reducer",
            "sweep/hollow-reducer",
            |k| hollow_reducer::build(k).map_err(crate::Refusal::from),
            Some((5119.999999999596, 5433.279594536134)),
        );
    }
    pub mod mitered_elbow;
    #[test]
    fn mitered_elbow() {
        crate::check(
            "cases::sweep::mitered_elbow",
            "sweep/mitered-elbow",
            |k| mitered_elbow::build(k).map_err(crate::Refusal::from),
            Some((8000.0, 3400.0)),
        );
    }
    pub mod mitered_picture_frame;
    #[test]
    fn mitered_picture_frame() {
        crate::check(
            "cases::sweep::mitered_picture_frame",
            "sweep/mitered-picture-frame",
            |k| mitered_picture_frame::build(k).map_err(crate::Refusal::from),
            Some((9864.0, 6544.0)),
        );
    }
    pub mod mitered_pipe_on_cube_edges;
    #[test]
    fn mitered_pipe_on_cube_edges() {
        crate::check(
            "cases::sweep::mitered_pipe_on_cube_edges",
            "sweep/mitered-pipe-on-cube-edges",
            |k| mitered_pipe_on_cube_edges::build(k).map_err(crate::Refusal::from),
            Some((1130.9733552923256, 1156.1060965210438)),
        );
    }
    pub mod narrowing_bend;
    #[test]
    fn narrowing_bend() {
        crate::check(
            "cases::sweep::narrowing_bend",
            "sweep/narrowing-bend",
            |k| narrowing_bend::build(k).map_err(crate::Refusal::from),
            Some((4900.88453960008, 5119.172836922541)),
        );
    }
    pub mod round_cornered_pipe;
    #[test]
    fn round_cornered_pipe() {
        crate::check(
            "cases::sweep::round_cornered_pipe",
            "sweep/round-cornered-pipe",
            |k| round_cornered_pipe::build(k).map_err(crate::Refusal::from),
            Some((1688.7343668207964, 1179.79635693925)),
        );
    }
    pub mod twisted_duct;
    #[test]
    fn twisted_duct() {
        crate::check(
            "cases::sweep::twisted_duct",
            "sweep/twisted-duct",
            |k| twisted_duct::build(k).map_err(crate::Refusal::from),
            Some((12373.333333333334, 3515.670710036986)),
        );
    }
}
pub mod swept {
    pub mod ball_along_spline_groove;
    #[test]
    fn ball_along_spline_groove() {
        crate::check(
            "cases::swept::ball_along_spline_groove",
            "swept/ball-along-spline-groove",
            |k| ball_along_spline_groove::build(k).map_err(crate::Refusal::from),
            Some((10173.828705378295, 4026.6392331153256)),
        );
    }
    pub mod ball_end_mill_screwed_across_ring_sector;
    #[test]
    fn ball_end_mill_screwed_across_ring_sector() {
        crate::check(
            "cases::swept::ball_end_mill_screwed_across_ring_sector",
            "swept/ball-end-mill-screwed-across-ring-sector",
            |k| ball_end_mill_screwed_across_ring_sector::build(k).map_err(crate::Refusal::from),
            None,
        );
    }
    pub mod ball_on_a_leaning_stem_swung_partway;
    #[test]
    fn ball_on_a_leaning_stem_swung_partway() {
        crate::check(
            "cases::swept::ball_on_a_leaning_stem_swung_partway",
            "swept/ball-on-a-leaning-stem-swung-partway",
            |k| ball_on_a_leaning_stem_swung_partway::build(k).map_err(crate::Refusal::from),
            None,
        );
    }
    pub mod ball_screwed_round_a_tight_helix;
    #[test]
    fn ball_screwed_round_a_tight_helix() {
        crate::check(
            "cases::swept::ball_screwed_round_a_tight_helix",
            "swept/ball-screwed-round-a-tight-helix",
            |k| ball_screwed_round_a_tight_helix::build(k).map_err(crate::Refusal::from),
            None,
        );
    }
    pub mod ball_swung_across_its_own_center_line;
    #[test]
    fn ball_swung_across_its_own_center_line() {
        crate::check(
            "cases::swept::ball_swung_across_its_own_center_line",
            "swept/ball-swung-across-its-own-center-line",
            |k| ball_swung_across_its_own_center_line::build(k).map_err(crate::Refusal::from),
            None,
        );
    }
    pub mod ball_swung_groove;
    #[test]
    fn ball_swung_groove() {
        crate::check(
            "cases::swept::ball_swung_groove",
            "swept/ball-swung-groove",
            |k| ball_swung_groove::build(k).map_err(crate::Refusal::from),
            Some((4284.972013405971, 2012.829877572684)),
        );
    }
    pub mod ball_thread_groove;
    #[test]
    fn ball_thread_groove() {
        crate::check(
            "cases::swept::ball_thread_groove",
            "swept/ball-thread-groove",
            |k| ball_thread_groove::build(k).map_err(crate::Refusal::from),
            Some((1343.0088854765736, 880.5540322252776)),
        );
    }
    pub mod bar_end_turned_into_a_floor;
    #[test]
    fn bar_end_turned_into_a_floor() {
        crate::check(
            "cases::swept::bar_end_turned_into_a_floor",
            "swept/bar-end-turned-into-a-floor",
            |k| bar_end_turned_into_a_floor::build(k).map_err(crate::Refusal::from),
            None,
        );
    }
    pub mod bar_plunged_aslant_into_plate;
    #[test]
    fn bar_plunged_aslant_into_plate() {
        crate::check(
            "cases::swept::bar_plunged_aslant_into_plate",
            "swept/bar-plunged-aslant-into-plate",
            |k| bar_plunged_aslant_into_plate::build(k).map_err(crate::Refusal::from),
            Some((764.8696076308298, 681.245048696171)),
        );
    }
    pub mod bar_swung_partway_into_ring_sector;
    #[test]
    fn bar_swung_partway_into_ring_sector() {
        crate::check(
            "cases::swept::bar_swung_partway_into_ring_sector",
            "swept/bar-swung-partway-into-ring-sector",
            |k| bar_swung_partway_into_ring_sector::build(k).map_err(crate::Refusal::from),
            Some((76.49537612892978, 168.18615524888082)),
        );
    }
    pub mod bar_turned_facing_the_axis;
    #[test]
    fn bar_turned_facing_the_axis() {
        crate::check(
            "cases::swept::bar_turned_facing_the_axis",
            "swept/bar-turned-facing-the-axis",
            |k| bar_turned_facing_the_axis::build(k).map_err(crate::Refusal::from),
            Some((37.74535410823478, 84.88108660518738)),
        );
    }
    pub mod box_screwed_inside_a_rod;
    #[test]
    fn box_screwed_inside_a_rod() {
        crate::check(
            "cases::swept::box_screwed_inside_a_rod",
            "swept/box-screwed-inside-a-rod",
            |k| box_screwed_inside_a_rod::build(k).map_err(crate::Refusal::from),
            None,
        );
    }
    pub mod box_slid_aslant;
    #[test]
    fn box_slid_aslant() {
        crate::check(
            "cases::swept::box_slid_aslant",
            "swept/box-slid-aslant",
            |k| box_slid_aslant::build(k).map_err(crate::Refusal::from),
            Some((812.0, 542.6189318971692)),
        );
    }
    pub mod bull_nose_along_a_looping_spline;
    #[test]
    fn bull_nose_along_a_looping_spline() {
        crate::check(
            "cases::swept::bull_nose_along_a_looping_spline",
            "swept/bull-nose-along-a-looping-spline",
            |k| bull_nose_along_a_looping_spline::build(k).map_err(crate::Refusal::from),
            None,
        );
    }
    pub mod bull_nose_along_line_arc_line;
    #[test]
    fn bull_nose_along_line_arc_line() {
        crate::check(
            "cases::swept::bull_nose_along_line_arc_line",
            "swept/bull-nose-along-line-arc-line",
            |k| bull_nose_along_line_arc_line::build(k).map_err(crate::Refusal::from),
            Some((24653.516723445, 7159.255410440657)),
        );
    }
    pub mod bull_nose_carried_along_a_spline;
    #[test]
    fn bull_nose_carried_along_a_spline() {
        crate::check(
            "cases::swept::bull_nose_carried_along_a_spline",
            "swept/bull-nose-carried-along-a-spline",
            |k| bull_nose_carried_along_a_spline::build(k).map_err(crate::Refusal::from),
            None,
        );
    }
    pub mod bull_nose_pushed_in_at_a_lean;
    #[test]
    fn bull_nose_pushed_in_at_a_lean() {
        crate::check(
            "cases::swept::bull_nose_pushed_in_at_a_lean",
            "swept/bull-nose-pushed-in-at-a-lean",
            |k| bull_nose_pushed_in_at_a_lean::build(k).map_err(crate::Refusal::from),
            None,
        );
    }
    pub mod bull_nose_swung_partway_into_ring_sector;
    #[test]
    fn bull_nose_swung_partway_into_ring_sector() {
        crate::check(
            "cases::swept::bull_nose_swung_partway_into_ring_sector",
            "swept/bull-nose-swung-partway-into-ring-sector",
            |k| bull_nose_swung_partway_into_ring_sector::build(k).map_err(crate::Refusal::from),
            Some((71.99190095776234, 175.62853108881794)),
        );
    }
    pub mod coil_spring_pushed_aslant_through_plate;
    #[test]
    fn coil_spring_pushed_aslant_through_plate() {
        crate::check(
            "cases::swept::coil_spring_pushed_aslant_through_plate",
            "swept/coil-spring-pushed-aslant-through-plate",
            |k| coil_spring_pushed_aslant_through_plate::build(k).map_err(crate::Refusal::from),
            None,
        );
    }
    pub mod coin_turned_about_a_line_through_its_rim;
    #[test]
    fn coin_turned_about_a_line_through_its_rim() {
        crate::check(
            "cases::swept::coin_turned_about_a_line_through_its_rim",
            "swept/coin-turned-about-a-line-through-its-rim",
            |k| coin_turned_about_a_line_through_its_rim::build(k).map_err(crate::Refusal::from),
            None,
        );
    }
    pub mod cone_point_screwed_through_rod;
    #[test]
    fn cone_point_screwed_through_rod() {
        crate::check(
            "cases::swept::cone_point_screwed_through_rod",
            "swept/cone-point-screwed-through-rod",
            |k| cone_point_screwed_through_rod::build(k).map_err(crate::Refusal::from),
            None,
        );
    }
    pub mod cone_spun_about_a_line_through_it;
    #[test]
    fn cone_spun_about_a_line_through_it() {
        crate::check(
            "cases::swept::cone_spun_about_a_line_through_it",
            "swept/cone-spun-about-a-line-through-it",
            |k| cone_spun_about_a_line_through_it::build(k).map_err(crate::Refusal::from),
            None,
        );
    }
    pub mod core_drill_pushed_aslant_through_plate;
    #[test]
    fn core_drill_pushed_aslant_through_plate() {
        crate::check(
            "cases::swept::core_drill_pushed_aslant_through_plate",
            "swept/core-drill-pushed-aslant-through-plate",
            |k| core_drill_pushed_aslant_through_plate::build(k).map_err(crate::Refusal::from),
            Some((1541.958420344505, 1207.0623694832425)),
        );
    }
    pub mod creased_bar_turned_through_ring_sector;
    #[test]
    fn creased_bar_turned_through_ring_sector() {
        crate::check(
            "cases::swept::creased_bar_turned_through_ring_sector",
            "swept/creased-bar-turned-through-ring-sector",
            |k| creased_bar_turned_through_ring_sector::build(k).map_err(crate::Refusal::from),
            None,
        );
    }
    pub mod cube_spun_on_its_diagonal_inside_a_block;
    #[test]
    fn cube_spun_on_its_diagonal_inside_a_block() {
        crate::check(
            "cases::swept::cube_spun_on_its_diagonal_inside_a_block",
            "swept/cube-spun-on-its-diagonal-inside-a-block",
            |k| cube_spun_on_its_diagonal_inside_a_block::build(k).map_err(crate::Refusal::from),
            Some((201.48960508612626, 248.29761691689097)),
        );
    }
    pub mod domed_mill_swung_partway_into_ring_sector;
    #[test]
    fn domed_mill_swung_partway_into_ring_sector() {
        crate::check(
            "cases::swept::domed_mill_swung_partway_into_ring_sector",
            "swept/domed-mill-swung-partway-into-ring-sector",
            |k| domed_mill_swung_partway_into_ring_sector::build(k).map_err(crate::Refusal::from),
            Some((63.21295221311727, 159.70096803807783)),
        );
    }
    pub mod drafted_slot_screwed_through_rod;
    #[test]
    fn drafted_slot_screwed_through_rod() {
        crate::check(
            "cases::swept::drafted_slot_screwed_through_rod",
            "swept/drafted-slot-screwed-through-rod",
            |k| drafted_slot_screwed_through_rod::build(k).map_err(crate::Refusal::from),
            None,
        );
    }
    pub mod drill_pushed_through_plate;
    #[test]
    fn drill_pushed_through_plate() {
        crate::check(
            "cases::swept::drill_pushed_through_plate",
            "swept/drill-pushed-through-plate",
            |k| drill_pushed_through_plate::build(k).map_err(crate::Refusal::from),
            Some((749.7345175425633, 665.1327412287184)),
        );
    }
    pub mod drill_swung_partway_into_ring_sector;
    #[test]
    fn drill_swung_partway_into_ring_sector() {
        crate::check(
            "cases::swept::drill_swung_partway_into_ring_sector",
            "swept/drill-swung-partway-into-ring-sector",
            |k| drill_swung_partway_into_ring_sector::build(k).map_err(crate::Refusal::from),
            Some((65.87413339909504, 156.99586044887178)),
        );
    }
    pub mod drum_end_screwed_through_rod;
    #[test]
    fn drum_end_screwed_through_rod() {
        crate::check(
            "cases::swept::drum_end_screwed_through_rod",
            "swept/drum-end-screwed-through-rod",
            |k| drum_end_screwed_through_rod::build(k).map_err(crate::Refusal::from),
            None,
        );
    }
    pub mod drum_leaning_across_a_turn;
    #[test]
    fn drum_leaning_across_a_turn() {
        crate::check(
            "cases::swept::drum_leaning_across_a_turn",
            "swept/drum-leaning-across-a-turn",
            |k| drum_leaning_across_a_turn::build(k).map_err(crate::Refusal::from),
            None,
        );
    }
    pub mod drum_leaning_to_a_screw;
    #[test]
    fn drum_leaning_to_a_screw() {
        crate::check(
            "cases::swept::drum_leaning_to_a_screw",
            "swept/drum-leaning-to-a-screw",
            |k| drum_leaning_to_a_screw::build(k).map_err(crate::Refusal::from),
            None,
        );
    }
    pub mod drum_plunged_aslant_into_plate;
    #[test]
    fn drum_plunged_aslant_into_plate() {
        crate::check(
            "cases::swept::drum_plunged_aslant_into_plate",
            "swept/drum-plunged-aslant-into-plate",
            |k| drum_plunged_aslant_into_plate::build(k).map_err(crate::Refusal::from),
            None,
        );
    }
    pub mod drum_pushed_through_plate;
    #[test]
    fn drum_pushed_through_plate() {
        crate::check(
            "cases::swept::drum_pushed_through_plate",
            "swept/drum-pushed-through-plate",
            |k| drum_pushed_through_plate::build(k).map_err(crate::Refusal::from),
            Some((672.0, 672.0)),
        );
    }
    pub mod drum_screwed_across_ring_sector;
    #[test]
    fn drum_screwed_across_ring_sector() {
        crate::check(
            "cases::swept::drum_screwed_across_ring_sector",
            "swept/drum-screwed-across-ring-sector",
            |k| drum_screwed_across_ring_sector::build(k).map_err(crate::Refusal::from),
            None,
        );
    }
    pub mod drum_swung_partway_into_ring_sector;
    #[test]
    fn drum_swung_partway_into_ring_sector() {
        crate::check(
            "cases::swept::drum_swung_partway_into_ring_sector",
            "swept/drum-swung-partway-into-ring-sector",
            |k| drum_swung_partway_into_ring_sector::build(k).map_err(crate::Refusal::from),
            Some((71.73303225696695, 177.36281798666926)),
        );
    }
    pub mod ellipsoid_pushed_aslant_through_plate;
    #[test]
    fn ellipsoid_pushed_aslant_through_plate() {
        crate::check(
            "cases::swept::ellipsoid_pushed_aslant_through_plate",
            "swept/ellipsoid-pushed-aslant-through-plate",
            |k| ellipsoid_pushed_aslant_through_plate::build(k).map_err(crate::Refusal::from),
            Some((3522.2654176407505, 2307.9842959944003)),
        );
    }
    pub mod end_mill_slid_across_block;
    #[test]
    fn end_mill_slid_across_block() {
        crate::check(
            "cases::swept::end_mill_slid_across_block",
            "swept/end-mill-slid-across-block",
            |k| end_mill_slid_across_block::build(k).map_err(crate::Refusal::from),
            Some((1760.0, 1096.0)),
        );
    }
    pub mod end_mill_turned_about_a_line_through_its_end;
    #[test]
    fn end_mill_turned_about_a_line_through_its_end() {
        crate::check(
            "cases::swept::end_mill_turned_about_a_line_through_its_end",
            "swept/end-mill-turned-about-a-line-through-its-end",
            |k| {
                end_mill_turned_about_a_line_through_its_end::build(k).map_err(crate::Refusal::from)
            },
            Some((1906.7513385990562, 1249.3562859134042)),
        );
    }
    pub mod fine_thread_with_sharp_crests;
    #[test]
    fn fine_thread_with_sharp_crests() {
        crate::check(
            "cases::swept::fine_thread_with_sharp_crests",
            "swept/fine-thread-with-sharp-crests",
            |k| fine_thread_with_sharp_crests::build(k).map_err(crate::Refusal::from),
            Some((1276.7029992078067, 916.1203964876111)),
        );
    }
    pub mod flat_ring_swung_partway_into_ring_sector;
    #[test]
    fn flat_ring_swung_partway_into_ring_sector() {
        crate::check(
            "cases::swept::flat_ring_swung_partway_into_ring_sector",
            "swept/flat-ring-swung-partway-into-ring-sector",
            |k| flat_ring_swung_partway_into_ring_sector::build(k).map_err(crate::Refusal::from),
            None,
        );
    }
    pub mod frustum_screwed_across_ring_sector;
    #[test]
    fn frustum_screwed_across_ring_sector() {
        crate::check(
            "cases::swept::frustum_screwed_across_ring_sector",
            "swept/frustum-screwed-across-ring-sector",
            |k| frustum_screwed_across_ring_sector::build(k).map_err(crate::Refusal::from),
            None,
        );
    }
    pub mod half_drum_swung_partway_into_ring_sector;
    #[test]
    fn half_drum_swung_partway_into_ring_sector() {
        crate::check(
            "cases::swept::half_drum_swung_partway_into_ring_sector",
            "swept/half-drum-swung-partway-into-ring-sector",
            |k| half_drum_swung_partway_into_ring_sector::build(k).map_err(crate::Refusal::from),
            Some((73.30382858376184, 173.07963267948966)),
        );
    }
    pub mod key_carried_along_a_spline;
    #[test]
    fn key_carried_along_a_spline() {
        crate::check(
            "cases::swept::key_carried_along_a_spline",
            "swept/key-carried-along-a-spline",
            |k| key_carried_along_a_spline::build(k).map_err(crate::Refusal::from),
            None,
        );
    }
    pub mod pin_swung_through_ring_sector;
    #[test]
    fn pin_swung_through_ring_sector() {
        crate::check(
            "cases::swept::pin_swung_through_ring_sector",
            "swept/pin-swung-through-ring-sector",
            |k| pin_swung_through_ring_sector::build(k).map_err(crate::Refusal::from),
            Some((53.84340742402507, 198.4923077874897)),
        );
    }
    pub mod plus_cut_wheel;
    #[test]
    fn plus_cut_wheel() {
        crate::check(
            "cases::swept::plus_cut_wheel",
            "swept/plus-cut-wheel",
            |k| plus_cut_wheel::build(k).map_err(crate::Refusal::from),
            None,
        );
    }
    pub mod rack_cut_tooth_space;
    #[test]
    fn rack_cut_tooth_space() {
        crate::check(
            "cases::swept::rack_cut_tooth_space",
            "swept/rack-cut-tooth-space",
            |k| rack_cut_tooth_space::build(k).map_err(crate::Refusal::from),
            None,
        );
    }
    pub mod ring_carried_along_a_spline;
    #[test]
    fn ring_carried_along_a_spline() {
        crate::check(
            "cases::swept::ring_carried_along_a_spline",
            "swept/ring-carried-along-a-spline",
            |k| ring_carried_along_a_spline::build(k).map_err(crate::Refusal::from),
            None,
        );
    }
    pub mod ring_lying_flat_turned_whole;
    #[test]
    fn ring_lying_flat_turned_whole() {
        crate::check(
            "cases::swept::ring_lying_flat_turned_whole",
            "swept/ring-lying-flat-turned-whole",
            |k| ring_lying_flat_turned_whole::build(k).map_err(crate::Refusal::from),
            Some((761.0994599066701, 919.0131303240997)),
        );
    }
    pub mod ring_pushed_in_at_a_lean;
    #[test]
    fn ring_pushed_in_at_a_lean() {
        crate::check(
            "cases::swept::ring_pushed_in_at_a_lean",
            "swept/ring-pushed-in-at-a-lean",
            |k| ring_pushed_in_at_a_lean::build(k).map_err(crate::Refusal::from),
            None,
        );
    }
    pub mod ring_pushed_in_past_its_fold;
    #[test]
    fn ring_pushed_in_past_its_fold() {
        crate::check(
            "cases::swept::ring_pushed_in_past_its_fold",
            "swept/ring-pushed-in-past-its-fold",
            |k| ring_pushed_in_past_its_fold::build(k).map_err(crate::Refusal::from),
            None,
        );
    }
    pub mod ring_slid_flat_past_its_hole;
    #[test]
    fn ring_slid_flat_past_its_hole() {
        crate::check(
            "cases::swept::ring_slid_flat_past_its_hole",
            "swept/ring-slid-flat-past-its-hole",
            |k| ring_slid_flat_past_its_hole::build(k).map_err(crate::Refusal::from),
            Some((1631.2657285967537, 1061.826218357434)),
        );
    }
    pub mod ring_slid_flat_short_of_its_hole;
    #[test]
    fn ring_slid_flat_short_of_its_hole() {
        crate::check(
            "cases::swept::ring_slid_flat_short_of_its_hole",
            "swept/ring-slid-flat-short-of-its-hole",
            |k| ring_slid_flat_short_of_its_hole::build(k).map_err(crate::Refusal::from),
            None,
        );
    }
    pub mod rod_spun_about_a_line_through_it;
    #[test]
    fn rod_spun_about_a_line_through_it() {
        crate::check(
            "cases::swept::rod_spun_about_a_line_through_it",
            "swept/rod-spun-about-a-line-through-it",
            |k| rod_spun_about_a_line_through_it::build(k).map_err(crate::Refusal::from),
            None,
        );
    }
    pub mod rounded_block_slid_aslant;
    #[test]
    fn rounded_block_slid_aslant() {
        crate::check(
            "cases::swept::rounded_block_slid_aslant",
            "swept/rounded-block-slid-aslant",
            |k| rounded_block_slid_aslant::build(k).map_err(crate::Refusal::from),
            Some((694.6544527304435, 474.11451085738196)),
        );
    }
    pub mod short_bar_screwed_through_rod;
    #[test]
    fn short_bar_screwed_through_rod() {
        crate::check(
            "cases::swept::short_bar_screwed_through_rod",
            "swept/short-bar-screwed-through-rod",
            |k| short_bar_screwed_through_rod::build(k).map_err(crate::Refusal::from),
            Some((1326.8635603592684, 882.290972524117)),
        );
    }
    pub mod spheroid_wheel_screwed_through_rod;
    #[test]
    fn spheroid_wheel_screwed_through_rod() {
        crate::check(
            "cases::swept::spheroid_wheel_screwed_through_rod",
            "swept/spheroid-wheel-screwed-through-rod",
            |k| spheroid_wheel_screwed_through_rod::build(k).map_err(crate::Refusal::from),
            None,
        );
    }
    pub mod spline_bar_screwed_through_rod;
    #[test]
    fn spline_bar_screwed_through_rod() {
        crate::check(
            "cases::swept::spline_bar_screwed_through_rod",
            "swept/spline-bar-screwed-through-rod",
            |k| spline_bar_screwed_through_rod::build(k).map_err(crate::Refusal::from),
            None,
        );
    }
    pub mod square_bar_on_its_edge_swung;
    #[test]
    fn square_bar_on_its_edge_swung() {
        crate::check(
            "cases::swept::square_bar_on_its_edge_swung",
            "swept/square-bar-on-its-edge-swung",
            |k| square_bar_on_its_edge_swung::build(k).map_err(crate::Refusal::from),
            Some((137.09733552923257, 181.8880169477256)),
        );
    }
    pub mod square_bar_screwed_partway_up_rod;
    #[test]
    fn square_bar_screwed_partway_up_rod() {
        crate::check(
            "cases::swept::square_bar_screwed_partway_up_rod",
            "swept/square-bar-screwed-partway-up-rod",
            |k| square_bar_screwed_partway_up_rod::build(k).map_err(crate::Refusal::from),
            None,
        );
    }
    pub mod square_bar_screwed_through_rod;
    #[test]
    fn square_bar_screwed_through_rod() {
        crate::check(
            "cases::swept::square_bar_screwed_through_rod",
            "swept/square-bar-screwed-through-rod",
            |k| square_bar_screwed_through_rod::build(k).map_err(crate::Refusal::from),
            Some((1300.5502131828669, 821.1671555638915)),
        );
    }
    pub mod square_on_a_planet_carrier;
    #[test]
    fn square_on_a_planet_carrier() {
        crate::check(
            "cases::swept::square_on_a_planet_carrier",
            "swept/square-on-a-planet-carrier",
            |k| square_on_a_planet_carrier::build(k).map_err(crate::Refusal::from),
            None,
        );
    }
    pub mod square_thread_at_a_growing_lead;
    #[test]
    fn square_thread_at_a_growing_lead() {
        crate::check(
            "cases::swept::square_thread_at_a_growing_lead",
            "swept/square-thread-at-a-growing-lead",
            |k| square_thread_at_a_growing_lead::build(k).map_err(crate::Refusal::from),
            None,
        );
    }
    pub mod square_thread_with_a_run_out;
    #[test]
    fn square_thread_with_a_run_out() {
        crate::check(
            "cases::swept::square_thread_with_a_run_out",
            "swept/square-thread-with-a-run-out",
            |k| square_thread_with_a_run_out::build(k).map_err(crate::Refusal::from),
            None,
        );
    }
    pub mod tetrahedron_screwed;
    #[test]
    fn tetrahedron_screwed() {
        crate::check(
            "cases::swept::tetrahedron_screwed",
            "swept/tetrahedron-screwed",
            |k| tetrahedron_screwed::build(k).map_err(crate::Refusal::from),
            None,
        );
    }
    pub mod tilted_box_pushed_aslant_through_plate;
    #[test]
    fn tilted_box_pushed_aslant_through_plate() {
        crate::check(
            "cases::swept::tilted_box_pushed_aslant_through_plate",
            "swept/tilted-box-pushed-aslant-through-plate",
            |k| tilted_box_pushed_aslant_through_plate::build(k).map_err(crate::Refusal::from),
            Some((3546.3703703703704, 2309.917225658577)),
        );
    }
    pub mod torus_screwed_through_rod;
    #[test]
    fn torus_screwed_through_rod() {
        crate::check(
            "cases::swept::torus_screwed_through_rod",
            "swept/torus-screwed-through-rod",
            |k| torus_screwed_through_rod::build(k).map_err(crate::Refusal::from),
            None,
        );
    }
    pub mod torus_slid_through_rod;
    #[test]
    fn torus_slid_through_rod() {
        crate::check(
            "cases::swept::torus_slid_through_rod",
            "swept/torus-slid-through-rod",
            |k| torus_slid_through_rod::build(k).map_err(crate::Refusal::from),
            Some((1493.7545729515407, 794.3608989284018)),
        );
    }
    pub mod torus_turned_across_block;
    #[test]
    fn torus_turned_across_block() {
        crate::check(
            "cases::swept::torus_turned_across_block",
            "swept/torus-turned-across-block",
            |k| torus_turned_across_block::build(k).map_err(crate::Refusal::from),
            None,
        );
    }
    pub mod tumbling_cube;
    #[test]
    fn tumbling_cube() {
        crate::check(
            "cases::swept::tumbling_cube",
            "swept/tumbling-cube",
            |k| tumbling_cube::build(k).map_err(crate::Refusal::from),
            None,
        );
    }
    pub mod tumbling_torus;
    #[test]
    fn tumbling_torus() {
        crate::check(
            "cases::swept::tumbling_torus",
            "swept/tumbling-torus",
            |k| tumbling_torus::build(k).map_err(crate::Refusal::from),
            None,
        );
    }
    pub mod tumbling_torus_crossed_difference;
    #[test]
    fn tumbling_torus_crossed_difference() {
        crate::check(
            "cases::swept::tumbling_torus_crossed_difference",
            "swept/tumbling-torus-crossed-difference",
            |k| tumbling_torus_crossed_difference::build(k).map_err(crate::Refusal::from),
            None,
        );
    }
    pub mod tumbling_torus_crossed_forward;
    #[test]
    fn tumbling_torus_crossed_forward() {
        crate::check(
            "cases::swept::tumbling_torus_crossed_forward",
            "swept/tumbling-torus-crossed-forward",
            |k| tumbling_torus_crossed_forward::build(k).map_err(crate::Refusal::from),
            None,
        );
    }
    pub mod tumbling_torus_crossed_intersection;
    #[test]
    fn tumbling_torus_crossed_intersection() {
        crate::check(
            "cases::swept::tumbling_torus_crossed_intersection",
            "swept/tumbling-torus-crossed-intersection",
            |k| tumbling_torus_crossed_intersection::build(k).map_err(crate::Refusal::from),
            None,
        );
    }
    pub mod tumbling_torus_crossed_reverse;
    #[test]
    fn tumbling_torus_crossed_reverse() {
        crate::check(
            "cases::swept::tumbling_torus_crossed_reverse",
            "swept/tumbling-torus-crossed-reverse",
            |k| tumbling_torus_crossed_reverse::build(k).map_err(crate::Refusal::from),
            None,
        );
    }
    pub mod tumbling_torus_crossed_union;
    #[test]
    fn tumbling_torus_crossed_union() {
        crate::check(
            "cases::swept::tumbling_torus_crossed_union",
            "swept/tumbling-torus-crossed-union",
            |k| tumbling_torus_crossed_union::build(k).map_err(crate::Refusal::from),
            None,
        );
    }
    pub mod tumbling_torus_full_turn;
    #[test]
    fn tumbling_torus_full_turn() {
        crate::check(
            "cases::swept::tumbling_torus_full_turn",
            "swept/tumbling-torus-full-turn",
            |k| tumbling_torus_full_turn::build(k).map_err(crate::Refusal::from),
            None,
        );
    }
    pub mod tumbling_torus_half_turn;
    #[test]
    fn tumbling_torus_half_turn() {
        crate::check(
            "cases::swept::tumbling_torus_half_turn",
            "swept/tumbling-torus-half-turn",
            |k| tumbling_torus_half_turn::build(k).map_err(crate::Refusal::from),
            None,
        );
    }
    pub mod tumbling_torus_near_orthogonal;
    #[test]
    fn tumbling_torus_near_orthogonal() {
        crate::check(
            "cases::swept::tumbling_torus_near_orthogonal",
            "swept/tumbling-torus-near-orthogonal",
            |k| tumbling_torus_near_orthogonal::build(k).map_err(crate::Refusal::from),
            None,
        );
    }
    pub mod tumbling_torus_near_orthogonal_thick;
    #[test]
    fn tumbling_torus_near_orthogonal_thick() {
        crate::check(
            "cases::swept::tumbling_torus_near_orthogonal_thick",
            "swept/tumbling-torus-near-orthogonal-thick",
            |k| tumbling_torus_near_orthogonal_thick::build(k).map_err(crate::Refusal::from),
            None,
        );
    }
    pub mod tumbling_torus_thin;
    #[test]
    fn tumbling_torus_thin() {
        crate::check(
            "cases::swept::tumbling_torus_thin",
            "swept/tumbling-torus-thin",
            |k| tumbling_torus_thin::build(k).map_err(crate::Refusal::from),
            None,
        );
    }
    pub mod two_beads_slid_through_rod;
    #[test]
    fn two_beads_slid_through_rod() {
        crate::check(
            "cases::swept::two_beads_slid_through_rod",
            "swept/two-beads-slid-through-rod",
            |k| two_beads_slid_through_rod::build(k).map_err(crate::Refusal::from),
            Some((1486.4498667518676, 787.2349038041657)),
        );
    }
    pub mod v_bit_engraving_stopped;
    #[test]
    fn v_bit_engraving_stopped() {
        crate::check(
            "cases::swept::v_bit_engraving_stopped",
            "swept/v-bit-engraving-stopped",
            |k| v_bit_engraving_stopped::build(k).map_err(crate::Refusal::from),
            Some((5931.622419590427, 2230.05797488066)),
        );
    }
    pub mod v_wheel_screwed_through_rod;
    #[test]
    fn v_wheel_screwed_through_rod() {
        crate::check(
            "cases::swept::v_wheel_screwed_through_rod",
            "swept/v-wheel-screwed-through-rod",
            |k| v_wheel_screwed_through_rod::build(k).map_err(crate::Refusal::from),
            None,
        );
    }
    pub mod v_wheel_slid_across_block;
    #[test]
    fn v_wheel_slid_across_block() {
        crate::check(
            "cases::swept::v_wheel_slid_across_block",
            "swept/v-wheel-slid-across-block",
            |k| v_wheel_slid_across_block::build(k).map_err(crate::Refusal::from),
            Some((1953.81197846483, 1041.569219381653)),
        );
    }
    pub mod v_wheel_turned_once_round_rod;
    #[test]
    fn v_wheel_turned_once_round_rod() {
        crate::check(
            "cases::swept::v_wheel_turned_once_round_rod",
            "swept/v-wheel-turned-once-round-rod",
            |k| v_wheel_turned_once_round_rod::build(k).map_err(crate::Refusal::from),
            None,
        );
    }
}
pub mod tangent {
    pub mod ball_in_a_paraboloid_cup;
    #[test]
    fn ball_in_a_paraboloid_cup() {
        crate::check(
            "cases::tangent::ball_in_a_paraboloid_cup",
            "tangent/ball-in-a-paraboloid-cup",
            |k| ball_in_a_paraboloid_cup::build(k).map_err(crate::Refusal::from),
            Some((654.4984694978735, 455.32947609453726)),
        );
    }
    pub mod ball_in_band;
    #[test]
    fn ball_in_band() {
        crate::check(
            "cases::tangent::ball_in_band",
            "tangent/ball-in-band",
            |k| ball_in_band::build(k).map_err(crate::Refusal::from),
            Some((580.1474433629152, 370.7079331235956)),
        );
    }
    pub mod bore_a_micron_from_the_wall;
    #[test]
    fn bore_a_micron_from_the_wall() {
        crate::check(
            "cases::tangent::bore_a_micron_from_the_wall",
            "tangent/bore-a-micron-from-the-wall",
            |k| bore_a_micron_from_the_wall::build(k).map_err(crate::Refusal::from),
            Some((282.7433388230814, 622.0353454107791)),
        );
    }
    pub mod coaxial_overlap;
    #[test]
    fn coaxial_overlap() {
        crate::check(
            "cases::tangent::coaxial_overlap",
            "tangent/coaxial-overlap",
            |k| coaxial_overlap::build(k).map_err(crate::Refusal::from),
            Some((1256.6370614359173, 659.7344572538566)),
        );
    }
    pub mod cone_tip_on_face;
    #[test]
    fn cone_tip_on_face() {
        crate::check(
            "cases::tangent::cone_tip_on_face",
            "tangent/cone-tip-on-face",
            |k| cone_tip_on_face::build(k).map_err(crate::Refusal::from),
            Some((4276.343798232436, 1781.6136238803595)),
        );
    }
    pub mod cube_turned_a_hair;
    #[test]
    fn cube_turned_a_hair() {
        crate::check(
            "cases::tangent::cube_turned_a_hair",
            "tangent/cube-turned-a-hair",
            |k| cube_turned_a_hair::build(k).map_err(crate::Refusal::from),
            Some((1000.0087264939527, 600.0052358963717)),
        );
    }
    pub mod cylinder_inscribed_through_a_cube;
    #[test]
    fn cylinder_inscribed_through_a_cube() {
        crate::check(
            "cases::tangent::cylinder_inscribed_through_a_cube",
            "tangent/cylinder-inscribed-through-a-cube",
            |k| cylinder_inscribed_through_a_cube::build(k).map_err(crate::Refusal::from),
            Some((1785.3981633974483, 914.1592653589793)),
        );
    }
    pub mod grazing_cylinders;
    #[test]
    fn grazing_cylinders() {
        crate::check(
            "cases::tangent::grazing_cylinders",
            "tangent/grazing-cylinders",
            |k| grazing_cylinders::build(k).map_err(crate::Refusal::from),
            Some((0.0015700108780411634, 0.6281614113696172)),
        );
    }
    pub mod hairline_slot;
    #[test]
    fn hairline_slot() {
        crate::check(
            "cases::tangent::hairline_slot",
            "tangent/hairline-slot",
            |k| hairline_slot::build(k).map_err(crate::Refusal::from),
            Some((999.99995, 699.99999)),
        );
    }
    pub mod half_balls_overlapping;
    #[test]
    fn half_balls_overlapping() {
        crate::check(
            "cases::tangent::half_balls_overlapping",
            "tangent/half-balls-overlapping",
            |k| half_balls_overlapping::build(k).map_err(crate::Refusal::from),
            Some((392.69908169872417, 314.1592653589793)),
        );
    }
    pub mod half_tori_overlapping;
    #[test]
    fn half_tori_overlapping() {
        crate::check(
            "cases::tangent::half_tori_overlapping",
            "tangent/half-tori-overlapping",
            |k| half_tori_overlapping::build(k).map_err(crate::Refusal::from),
            Some((1332.3965941470633, 1105.034289195738)),
        );
    }
    pub mod ice_cream_cone;
    #[test]
    fn ice_cream_cone() {
        crate::check(
            "cases::tangent::ice_cream_cone",
            "tangent/ice-cream-cone",
            |k| ice_cream_cone::build(k).map_err(crate::Refusal::from),
            Some((15079.644737231007, 3015.928947446201)),
        );
    }
    pub mod inscribed_ball_common;
    #[test]
    fn inscribed_ball_common() {
        crate::check(
            "cases::tangent::inscribed_ball_common",
            "tangent/inscribed-ball-common",
            |k| inscribed_ball_common::build(k).map_err(crate::Refusal::from),
            Some((523.5987755982989, 314.1592653589793)),
        );
    }
    pub mod inscribed_ball_union;
    #[test]
    fn inscribed_ball_union() {
        crate::check(
            "cases::tangent::inscribed_ball_union",
            "tangent/inscribed-ball-union",
            |k| inscribed_ball_union::build(k).map_err(crate::Refusal::from),
            Some((1000.0, 600.0)),
        );
    }
    pub mod knife_edge_dish;
    #[test]
    fn knife_edge_dish() {
        crate::check(
            "cases::tangent::knife_edge_dish",
            "tangent/knife-edge-dish",
            |k| knife_edge_dish::build(k).map_err(crate::Refusal::from),
            Some((523.5987755982989, 549.7787143782137)),
        );
    }
    pub mod midsphere_common;
    #[test]
    fn midsphere_common() {
        crate::check(
            "cases::tangent::midsphere_common",
            "tangent/midsphere-common",
            |k| midsphere_common::build(k).map_err(crate::Refusal::from),
            Some((965.0688582149972, 547.4667180500616)),
        );
    }
    pub mod paraboloid_tip_a_ten_thousandth_high;
    #[test]
    fn paraboloid_tip_a_ten_thousandth_high() {
        crate::check(
            "cases::tangent::paraboloid_tip_a_ten_thousandth_high",
            "tangent/paraboloid-tip-a-ten-thousandth-high",
            |k| paraboloid_tip_a_ten_thousandth_high::build(k).map_err(crate::Refusal::from),
            Some((1.2566370614359172e-07, 0.0050265796614084104)),
        );
    }
    pub mod paraboloid_tip_on_face;
    #[test]
    fn paraboloid_tip_on_face() {
        crate::check(
            "cases::tangent::paraboloid_tip_on_face",
            "tangent/paraboloid-tip-on-face",
            |k| paraboloid_tip_on_face::build(k).map_err(crate::Refusal::from),
            Some((4706.858347057703, 1824.4951605529168)),
        );
    }
    pub mod pole_cap_a_ten_thousandth_high;
    #[test]
    fn pole_cap_a_ten_thousandth_high() {
        crate::check(
            "cases::tangent::pole_cap_a_ten_thousandth_high",
            "tangent/pole-cap-a-ten-thousandth-high",
            |k| pole_cap_a_ten_thousandth_high::build(k).map_err(crate::Refusal::from),
            Some((1.5707858548193846e-07, 0.00628315389125305)),
        );
    }
    pub mod tangent_tee_common;
    #[test]
    fn tangent_tee_common() {
        crate::check(
            "cases::tangent::tangent_tee_common",
            "tangent/tangent-tee-common",
            |k| tangent_tee_common::build(k).map_err(crate::Refusal::from),
            Some((239.74642882659614, 214.8559410549457)),
        );
    }
    pub mod tangent_tee_union;
    #[test]
    fn tangent_tee_union() {
        crate::check(
            "cases::tangent::tangent_tee_union",
            "tangent/tangent-tee-union",
            |k| tangent_tee_union::build(k).map_err(crate::Refusal::from),
            Some((2964.678077834993, 1506.736833112261)),
        );
    }
    pub mod thin_lens;
    #[test]
    fn thin_lens() {
        crate::check(
            "cases::tangent::thin_lens",
            "tangent/thin-lens",
            |k| thin_lens::build(k).map_err(crate::Refusal::from),
            Some((7.853981372175095e-12, 3.1415926535897935e-05)),
        );
    }
    pub mod torus_in_drum;
    #[test]
    fn torus_in_drum() {
        crate::check(
            "cases::tangent::torus_in_drum",
            "tangent/torus-in-drum",
            |k| torus_in_drum::build(k).map_err(crate::Refusal::from),
            Some((2098.609984319263, 1442.4813997670688)),
        );
    }
    pub mod torus_through_a_square_slab;
    #[test]
    fn torus_through_a_square_slab() {
        crate::check(
            "cases::tangent::torus_through_a_square_slab",
            "tangent/torus-through-a-square-slab",
            |k| torus_through_a_square_slab::build(k).map_err(crate::Refusal::from),
            Some((2388.7516674059125, 1777.2602648670495)),
        );
    }
    pub mod villarceau_half_torus;
    #[test]
    fn villarceau_half_torus() {
        crate::check(
            "cases::tangent::villarceau_half_torus",
            "tangent/villarceau-half-torus",
            |k| villarceau_half_torus::build(k).map_err(crate::Refusal::from),
            Some((444.13219804902116, 408.4382429120092)),
        );
    }
    pub mod viviani_common;
    #[test]
    fn viviani_common() {
        crate::check(
            "cases::tangent::viviani_common",
            "tangent/viviani-common",
            |k| viviani_common::build(k).map_err(crate::Refusal::from),
            Some((1205.5062135043065, 628.3185307179587)),
        );
    }
    pub mod viviani_cut;
    #[test]
    fn viviani_cut() {
        crate::check(
            "cases::tangent::viviani_cut",
            "tangent/viviani-cut",
            |k| viviani_cut::build(k).map_err(crate::Refusal::from),
            Some((2983.2839912820846, 1428.3185307179585)),
        );
    }
    pub mod viviani_union;
    #[test]
    fn viviani_union() {
        crate::check(
            "cases::tangent::viviani_union",
            "tangent/viviani-union",
            |k| viviani_union::build(k).map_err(crate::Refusal::from),
            Some((4868.23958343596, 1539.3804002589986)),
        );
    }
    pub mod wedge_a_millionth_steep;
    #[test]
    fn wedge_a_millionth_steep() {
        crate::check(
            "cases::tangent::wedge_a_millionth_steep",
            "tangent/wedge-a-millionth-steep",
            |k| wedge_a_millionth_steep::build(k).map_err(crate::Refusal::from),
            Some((0.000125, 100.000075000025)),
        );
    }
}
pub mod thicken {
    pub mod angle_bracket_inward;
    #[test]
    fn angle_bracket_inward() {
        crate::check(
            "cases::thicken::angle_bracket_inward",
            "thicken/angle-bracket-inward",
            |k| angle_bracket_inward::build(k).map_err(crate::Refusal::from),
            Some((152.0, 358.0)),
        );
    }
    pub mod angle_bracket_rounded;
    #[test]
    fn angle_bracket_rounded() {
        crate::check(
            "cases::thicken::angle_bracket_rounded",
            "thicken/angle-bracket-rounded",
            |k| angle_bracket_rounded::build(k).map_err(crate::Refusal::from),
            Some((166.2831853071796, 390.1371669411541)),
        );
    }
    pub mod angle_bracket_sharp;
    #[test]
    fn angle_bracket_sharp() {
        crate::check(
            "cases::thicken::angle_bracket_sharp",
            "thicken/angle-bracket-sharp",
            |k| angle_bracket_sharp::build(k).map_err(crate::Refusal::from),
            Some((168.0, 394.0)),
        );
    }
    pub mod bent_flange;
    #[test]
    fn bent_flange() {
        crate::check(
            "cases::thicken::bent_flange",
            "thicken/bent-flange",
            |k| bent_flange::build(k).map_err(crate::Refusal::from),
            Some((191.41592653589794, 446.68583470577033)),
        );
    }
    pub mod bent_flange_past_its_bend;
    #[test]
    fn bent_flange_past_its_bend() {
        crate::check(
            "cases::thicken::bent_flange_past_its_bend",
            "thicken/bent-flange-past-its-bend",
            |k| bent_flange_past_its_bend::build(k).map_err(crate::Refusal::from),
            Some((497.1327412287184, 501.41592653589794)),
        );
    }
    pub mod cone_frustum;
    #[test]
    fn cone_frustum() {
        crate::check(
            "cases::thicken::cone_frustum",
            "thicken/cone-frustum",
            |k| cone_frustum::build(k).map_err(crate::Refusal::from),
            Some((153.93804002589985, 369.45129606215966)),
        );
    }
    pub mod half_pipe_inward;
    #[test]
    fn half_pipe_inward() {
        crate::check(
            "cases::thicken::half_pipe_inward",
            "thicken/half-pipe-inward",
            |k| half_pipe_inward::build(k).map_err(crate::Refusal::from),
            Some((251.32741228718345, 341.59289474462014)),
        );
    }
    pub mod half_pipe_outward;
    #[test]
    fn half_pipe_outward() {
        crate::check(
            "cases::thicken::half_pipe_outward",
            "thicken/half-pipe-outward",
            |k| half_pipe_outward::build(k).map_err(crate::Refusal::from),
            Some((172.78759594743863, 400.132711084365)),
        );
    }
    pub mod hemisphere_both_sides;
    #[test]
    fn hemisphere_both_sides() {
        crate::check(
            "cases::thicken::hemisphere_both_sides",
            "thicken/hemisphere-both-sides",
            |k| hemisphere_both_sides::build(k).map_err(crate::Refusal::from),
            Some((318.3480555637657, 389.55748904513433)),
        );
    }
    pub mod hemisphere_to_its_center;
    #[test]
    fn hemisphere_to_its_center() {
        crate::check(
            "cases::thicken::hemisphere_to_its_center",
            "thicken/hemisphere-to-its-center",
            |k| hemisphere_to_its_center::build(k).map_err(crate::Refusal::from),
            Some((261.79938779914943, 235.61944901923448)),
        );
    }
    pub mod open_box_inward;
    #[test]
    fn open_box_inward() {
        crate::check(
            "cases::thicken::open_box_inward",
            "thicken/open-box-inward",
            |k| open_box_inward::build(k).map_err(crate::Refusal::from),
            Some((424.0, 888.0)),
        );
    }
    pub mod open_box_outward_rounded;
    #[test]
    fn open_box_outward_rounded() {
        crate::check(
            "cases::thicken::open_box_outward_rounded",
            "thicken/open-box-outward-rounded",
            |k| open_box_outward_rounded::build(k).map_err(crate::Refusal::from),
            Some((564.926248174189, 1175.0884841043612)),
        );
    }
    pub mod open_box_outward_sharp;
    #[test]
    fn open_box_outward_sharp() {
        crate::check(
            "cases::thicken::open_box_outward_sharp",
            "thicken/open-box-outward-sharp",
            |k| open_box_outward_sharp::build(k).map_err(crate::Refusal::from),
            Some((584.0, 1216.0)),
        );
    }
    pub mod parabolic_trough;
    #[test]
    fn parabolic_trough() {
        crate::check(
            "cases::thicken::parabolic_trough",
            "thicken/parabolic-trough",
            |k| parabolic_trough::build(k).map_err(crate::Refusal::from),
            Some((83.96950434173104, 204.7329095518083)),
        );
    }
    pub mod plate_with_holes;
    #[test]
    fn plate_with_holes() {
        crate::check(
            "cases::thicken::plate_with_holes",
            "thicken/plate-with-holes",
            |k| plate_with_holes::build(k).map_err(crate::Refusal::from),
            Some((686.9026644707675, 922.3008881569225)),
        );
    }
    pub mod rectangle;
    #[test]
    fn rectangle() {
        crate::check(
            "cases::thicken::rectangle",
            "thicken/rectangle",
            |k| rectangle::build(k).map_err(crate::Refusal::from),
            Some((160.0, 232.0)),
        );
    }
    pub mod sphere_skin;
    #[test]
    fn sphere_skin() {
        crate::check(
            "cases::thicken::sphere_skin",
            "thicken/sphere-skin",
            |k| sphere_skin::build(k).map_err(crate::Refusal::from),
            Some((381.17990863556156, 766.5486074759095)),
        );
    }
    pub mod spherical_cap;
    #[test]
    fn spherical_cap() {
        crate::check(
            "cases::thicken::spherical_cap",
            "thicken/spherical-cap",
            |k| spherical_cap::build(k).map_err(crate::Refusal::from),
            Some((609.8878538168984, 723.8229473870883)),
        );
    }
    pub mod torus_inner_half;
    #[test]
    fn torus_inner_half() {
        crate::check(
            "cases::thicken::torus_inner_half",
            "thicken/torus-inner-half",
            |k| torus_inner_half::build(k).map_err(crate::Refusal::from),
            Some((216.5011181417393, 504.2116697648473)),
        );
    }
    pub mod tube;
    #[test]
    fn tube() {
        crate::check(
            "cases::thicken::tube",
            "thicken/tube",
            |k| tube::build(k).map_err(crate::Refusal::from),
            Some((345.57519189487726, 760.26542216873)),
        );
    }
}
