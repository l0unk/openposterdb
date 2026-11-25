import type { TitleDetail, TitleSummary } from '@/louk/api'

export function summary(over: Partial<TitleSummary> = {}): TitleSummary {
  return {
    media_type: 'movie',
    tmdb_id: 603,
    name: 'The Matrix',
    year: 1999,
    imdb_id: 'tt0133093',
    poster_thumb_url: 'https://image.tmdb.org/t/p/w185/matrix.jpg',
    last_requested: Math.floor(Date.now() / 1000) - 7200,
    image_counts: { poster: 2, logo: 1, backdrop: 1, episode: 0 },
    ...over,
  }
}

export function showDetail(): TitleDetail {
  const img = (kind: 'poster' | 'backdrop' | 'logo' | 'episode', idType: string, value: string) => ({
    kind,
    id_type: idType,
    cache_value: value,
    created_at: Math.floor(Date.now() / 1000) - 3600,
    admin_image_url: `/api/admin/${kind}s/${idType}/${value}/image`,
  })
  return {
    ...summary({
      media_type: 'tv',
      tmdb_id: 1399,
      name: 'Game of Thrones',
      year: 2011,
      imdb_id: 'tt0944947',
      image_counts: { poster: 1, logo: 0, backdrop: 1, episode: 2 },
    }),
    tvdb_id: 121361,
    aliases: [
      { id_type: 'imdb', id_value: 'tt0944947' },
      { id_type: 'tmdb', id_value: 'series-1399' },
      { id_type: 'imdb', id_value: 'episode-tt0944947-S1E2', season: 1, episode: 2 },
    ],
    images: [
      img('poster', 'imdb', 'tt0944947@ir.pbc.zm'),
      img('backdrop', 'tmdb', 'series-1399@ir.zl'),
      img('episode', 'imdb', 'episode-tt0944947-S1E2@ir.zm'),
      img('episode', 'imdb', 'episode-tt0944947-S2E1@ir.zm'),
    ],
    seasons: [
      {
        season: 1,
        episodes: [
          { episode: 1, name: 'Winter Is Coming', still_thumb_url: 'https://image.tmdb.org/t/p/w185/s1e1.jpg', cached: [] },
          {
            episode: 2,
            name: 'The Kingsroad',
            still_thumb_url: null,
            cached: [img('episode', 'imdb', 'episode-tt0944947-S1E2@ir.zm')],
          },
        ],
      },
      {
        season: 2,
        episodes: [
          {
            episode: 1,
            name: 'The North Remembers',
            still_thumb_url: null,
            cached: [img('episode', 'imdb', 'episode-tt0944947-S2E1@ir.zm')],
          },
        ],
      },
    ],
  }
}
