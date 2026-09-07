# Scientific-format fixtures: provenance

`two-triangles.vtk` was written by hand for this project (its header line says
`gungnir testdata`): four points on a square, two triangles, one scalar per point, in
legacy VTK ASCII polydata. Nothing was copied from any dataset or example collection,
so no third-party licence attaches. It exercises the VTK loader in `gungnir-data` and
is never linked into or shipped with a binary.
