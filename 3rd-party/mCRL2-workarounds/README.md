# mcrl2_syntax

The `mcrl2_syntax.c` cannot be generated automatically by `mcrl2-sys` so it must
be compiled in the toolset and copied here from
`<build>/libraries/core/source/mcrl2_syntax.c`. This is only necessary when the
grammar of mCRL2 has been updated.

# toolset_version_const

This normally contains the proper toolset version, but since it cannot be
generated without cmake we simply put something in it.