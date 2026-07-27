import { PrismaClient } from '@prisma/client';

const prisma = new PrismaClient();

// Tax rates by country
export const TAX_RATES: Record<string, { rate: number; name: string }> = {
    // EU
    'DE': { rate: 19, name: 'VAT' },
    'FR': { rate: 20, name: 'VAT' },
    'ES': { rate: 21, name: 'VAT' },
    'IT': { rate: 22, name: 'VAT' },
    'NL': { rate: 21, name: 'VAT' },
    'BE': { rate: 21, name: 'VAT' },
    'AT': { rate: 20, name: 'VAT' },
    'IE': { rate: 23, name: 'VAT' },
    'PT': { rate: 23, name: 'VAT' },
    'GR': { rate: 24, name: 'VAT' },
    'FI': { rate: 24, name: 'VAT' },
    'SE': { rate: 25, name: 'VAT' },
    'DK': { rate: 25, name: 'VAT' },
    'PL': { rate: 23, name: 'VAT' },
    'CZ': { rate: 21, name: 'VAT' },
    'HU': { rate: 27, name: 'VAT' },
    'SK': { rate: 20, name: 'VAT' },
    'SI': { rate: 22, name: 'VAT' },
    'EE': { rate: 20, name: 'VAT' },
    'LV': { rate: 21, name: 'VAT' },
    'LT': { rate: 21, name: 'VAT' },
    'RO': { rate: 19, name: 'VAT' },
    'BG': { rate: 20, name: 'VAT' },
    'HR': { rate: 25, name: 'VAT' },
    'CY': { rate: 19, name: 'VAT' },
    'MT': { rate: 18, name: 'VAT' },
    'LU': { rate: 17, name: 'VAT' },

    // Other major markets
    'GB': { rate: 20, name: 'VAT' },
    'CH': { rate: 7.7, name: 'VAT' },
    'NO': { rate: 25, name: 'VAT' },
    'US': { rate: 0, name: 'Sales Tax' }, // Handled separately by state
    'CA': { rate: 5, name: 'GST' }, // Plus provincial
    'AU': { rate: 10, name: 'GST' },
    'NZ': { rate: 15, name: 'GST' },
    'JP': { rate: 10, name: 'Consumption Tax' },
    'SG': { rate: 9, name: 'GST' },
    'HK': { rate: 0, name: 'No Tax' },
    'KR': { rate: 10, name: 'VAT' },
    'IN': { rate: 18, name: 'GST' },
    'BR': { rate: 17, name: 'ICMS' },
    'MX': { rate: 16, name: 'IVA' },
    'ZA': { rate: 15, name: 'VAT' },
    'AE': { rate: 5, name: 'VAT' },
    'IL': { rate: 17, name: 'VAT' },
    'TR': { rate: 20, name: 'VAT' },
    'RU': { rate: 20, name: 'VAT' },
};

// US state sales tax rates (simplified)
export const US_STATE_TAX: Record<string, number> = {
    'AL': 4, 'AK': 0, 'AZ': 5.6, 'AR': 6.5, 'CA': 7.25,
    'CO': 2.9, 'CT': 6.35, 'DE': 0, 'FL': 6, 'GA': 4,
    'HI': 4, 'ID': 6, 'IL': 6.25, 'IN': 7, 'IA': 6,
    'KS': 6.5, 'KY': 6, 'LA': 4.45, 'ME': 5.5, 'MD': 6,
    'MA': 6.25, 'MI': 6, 'MN': 6.875, 'MS': 7, 'MO': 4.225,
    'MT': 0, 'NE': 5.5, 'NV': 6.85, 'NH': 0, 'NJ': 6.625,
    'NM': 5.125, 'NY': 4, 'NC': 4.75, 'ND': 5, 'OH': 5.75,
    'OK': 4.5, 'OR': 0, 'PA': 6, 'RI': 7, 'SC': 6,
    'SD': 4.5, 'TN': 7, 'TX': 6.25, 'UT': 6.1, 'VT': 6,
    'VA': 5.3, 'WA': 6.5, 'WV': 6, 'WI': 5, 'WY': 4,
    'DC': 6,
};

export class TaxService {
    calculateTax(amount: number, countryCode: string, stateCode?: string): {
        taxable: boolean;
        rate: number;
        amount: number;
        name: string;
        jurisdiction: string;
    } {
        // B2B software services often have special rules
        // This is simplified; real implementation needs nexus rules

        if (countryCode === 'US') {
            if (stateCode && US_STATE_TAX[stateCode]) {
                const rate = US_STATE_TAX[stateCode] / 100;
                return {
                    taxable: true,
                    rate,
                    amount: amount * rate,
                    name: 'Sales Tax',
                    jurisdiction: `${stateCode}, US`,
                };
            }
            return {
                taxable: false,
                rate: 0,
                amount: 0,
                name: 'No Tax',
                jurisdiction: 'US',
            };
        }

        const taxInfo = TAX_RATES[countryCode];
        if (taxInfo && taxInfo.rate > 0) {
            const rate = taxInfo.rate / 100;
            return {
                taxable: true,
                rate,
                amount: amount * rate,
                name: taxInfo.name,
                jurisdiction: countryCode,
            };
        }

        return {
            taxable: false,
            rate: 0,
            amount: 0,
            name: 'No Tax',
            jurisdiction: countryCode,
        };
    }

    // Validate VAT number (EU)
    async validateVATNumber(vatNumber: string, countryCode: string): Promise<boolean> {
        // Call EU VIES service
        try {
            const response = await fetch(
                `http://ec.europa.eu/taxation_customs/vies/rest-api/check-vat/${countryCode}/${vatNumber}`
            );
            const data = await response.json();
            return data.valid;
        } catch {
            return false;
        }
    }

    // Get reverse charge eligibility (B2B within EU)
    isReverseChargeEligible(sellerCountry: string, buyerCountry: string, buyerHasVAT: boolean): boolean {
        return (
            sellerCountry !== buyerCountry &&
            buyerHasVAT &&
            Object.keys(TAX_RATES).includes(buyerCountry)
        );
    }
}

export const taxService = new TaxService();
