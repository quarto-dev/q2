-- bd-sgfiiktn S3: table-part userdata with cache+readback. Nested
-- in-place mutation through caption/colspecs/head/bodies must
-- persist without any reassignment (hslua aliasing semantics).
function Table(tbl)
  tbl.caption.long = { pandoc.Plain 'fancy caption' }
  tbl.colspecs[1][1] = 'AlignCenter'
  tbl.head.rows[1].cells[1].contents = { pandoc.Plain 'HDR' }
  tbl.bodies[1].body[1].cells[2].contents = { pandoc.Plain 'X' }
  return tbl
end
