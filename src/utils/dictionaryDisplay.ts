export type ExchangeCode = 'p' | 'd' | 'i' | '3' | 'r' | 't' | 's' | '0';

/** ECDICT `1:` describes a transformation, not a word to look up. */
const EXCHANGE_CODES = new Set<string>(['p', 'd', 'i', '3', 'r', 't', 's', '0']);

export function exchangeItems(exchange: string): { code: ExchangeCode; value: string }[] {
  const result: { code: ExchangeCode; value: string }[] = [];
  const seen = new Set<string>();
  for (const item of exchange.split('/')) {
    const colon = item.indexOf(':');
    if (colon < 0) continue;
    const code = item.slice(0, colon).trim();
    const value = item.slice(colon + 1).trim();
    if (!value || !EXCHANGE_CODES.has(code) || seen.has(`${code}:${value}`)) continue;
    seen.add(`${code}:${value}`);
    result.push({ code: code as ExchangeCode, value });
  }
  return result;
}

export function phoneticParts(phonetic: string): { accent: 'uk' | 'us'; value: string }[] | null {
  const uk = phonetic.indexOf('英');
  const us = phonetic.indexOf('美');
  if (uk === -1 && us === -1) return null;
  const result: { accent: 'uk' | 'us'; value: string }[] = [];
  if (uk >= 0) {
    const value = phonetic.slice(uk + 1, us > uk ? us : phonetic.length).trim();
    if (value) result.push({ accent: 'uk', value });
  }
  if (us >= 0) {
    const value = phonetic.slice(us + 1, uk > us ? uk : phonetic.length).trim();
    if (value) result.push({ accent: 'us', value });
  }
  return result.length ? result : null;
}
