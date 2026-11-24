# Data

- `coastlines-110m.bin`: the world's coastlines at 1:110m, from [Natural Earth](https://www.naturalearthdata.com/)
  (`ne_110m_coastline`, public domain), for `world_map`. Packed small: for each line a `u16`
  point count, then each point as two `i16`s, longitude and latitude in hundredths of a degree,
  little-endian.
