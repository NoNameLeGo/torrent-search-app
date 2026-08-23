'use strict';

// TorrentDownload HTML scraper. InfoHash is embedded in the detail-page URL
// (second-to-last path segment), so no second request is needed.
const { createProvider, scrapeRows } = require('../lib/scraper');

const DOMAINS = [
  'https://torrentdownload.info',
  'https://torrentdownload.me',
];

function categoryFromRaw(raw) {
  const r = (raw || '').replace(/[^A-Za-z]+/g, '');
  switch (r) {
    case 'XXX': case 'XXXVideo': case 'XXXHDVideo': case 'XXXPictures':
    case 'Adult': case 'AdultPornHDVideo': case 'AdultPornPictures': case 'AdultPornVideo':
      return 'Porn';
    case 'Anime': case 'AnimeEnglishtranslated': case 'AnimeAnimeOther': return 'Anime';
    case 'Applications': case 'ApplicationsAndroid': case 'ApplicationsWindows': case 'Software': return 'Apps';
    case 'BooksAcademic': case 'BooksComics': case 'BooksEbooks': case 'BooksEducational':
    case 'BooksMagazines': case 'BooksFiction': case 'BooksNonfiction': case 'BooksTextbooks':
    case 'Ebooks': case 'OtherEbooks': case 'OtherComics': case 'AudioBooks': case 'AudioAudiobooks':
      return 'Books';
    case 'Games': case 'GamesWindows': return 'Games';
    case 'Movies': case 'MoviesAction': case 'MoviesConcerts': case 'MoviesCrime':
    case 'MoviesDocumentary': case 'MoviesDubbedMovies': case 'MoviesHighresMovies':
    case 'MoviesMusicvideos': case 'MoviesThriller': case 'VideoMovies': return 'Movies';
    case 'Music': case 'MusicHardrock': case 'MusicMp': case 'MusicFLAC': case 'MusicLossless':
    case 'MusicRB': case 'MusicTranceHouseDance': case 'VideoMusic': case 'AudioMusic': return 'Music';
    case 'TV': case 'TVBBC': case 'TVshows': case 'Television': return 'Series';
    default: return 'Other';
  }
}

module.exports = createProvider({
  id: 'torrentdownload', name: 'TorrentDownload',
  mirrors: DOMAINS,
  searchOn: (base, query, page) => scrapeRows(base, query, page, {
    id: 'torrentdownload',
    url: ({ base, query }) => `${base}/search?q=${encodeURIComponent(query)}`,
    rowSelector: 'div.wrapper > table.table2:last-of-type > tbody > tr',
    extract($, $row, base) {
      const $link = $row.find('td:nth-child(1) > div.tt-name > a').first();
      const name = $link.text().trim();
      const href = $link.attr('href');
      if (!name || !href) return null;
      const detailUrl = href.startsWith('http') ? href : `${base}${href}`;
      const parts = detailUrl.split('/').filter(Boolean);
      const infoHash = (parts[parts.length - 2] || '').toLowerCase();
      if (!infoHash) return null;
      return {
        name,
        size: $row.find('td:nth-child(3)').text(),
        seeders: $row.find('td:nth-child(4)').text(),
        leechers: $row.find('td:nth-child(5)').text(),
        date: $row.find('td:nth-child(2)').text(),
        infoHash,
        detailUrl,
        category: categoryFromRaw($row.find('td:nth-child(1) > div.tt-name > span').text()),
      };
    },
  }),
});