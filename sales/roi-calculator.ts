// ROI Calculator for sales prospects

export interface ROICalculation {
    inputs: {
        developers: number;
        monthlySalary: number;
        aiCodePercentage: number;
        currentDebtHours: number;
        violationReduction: number;
    };
    outputs: {
        annualSavings: number;
        timeSavedHours: number;
        productivityGain: number;
        debtReduction: number;
        paybackPeriodDays: number;
        fiveYearValue: number;
    };
}

export class ROICalculator {
    calculate(inputs: Partial<ROICalculation['inputs']>): ROICalculation {
        const defaults = {
            developers: 10,
            monthlySalary: 10000,
            aiCodePercentage: 40,
            currentDebtHours: 100,
            violationReduction: 70,
        };

        const params = { ...defaults, ...inputs };

        // Calculations
        const developerCostPerHour = params.monthlySalary / 160; // 160 working hours/month

        // Current cost of architectural debt
        const currentMonthlyDebtCost = params.currentDebtHours * params.developers * developerCostPerHour;

        // Savings from CKB
        const monthlySavings = currentMonthlyDebtCost * (params.violationReduction / 100);
        const annualSavings = monthlySavings * 12;

        // Time saved
        const timeSavedHours = params.currentDebtHours * params.developers * (params.violationReduction / 100);

        // Productivity gain (as % of total development time)
        const totalMonthlyHours = params.developers * 160;
        const productivityGain = (timeSavedHours / totalMonthlyHours) * 100;

        // Debt reduction
        const debtReduction = params.violationReduction;

        // Payback period (based on Pro plan $29/user/month)
        const monthlyCkbCost = params.developers * 29;
        const paybackPeriodDays = (monthlyCkbCost / monthlySavings) * 30;

        // 5-year value with compounding
        const fiveYearValue = annualSavings * 5 * 1.2; // 20% growth assumption

        return {
            inputs: params,
            outputs: {
                annualSavings: Math.round(annualSavings),
                timeSavedHours: Math.round(timeSavedHours),
                productivityGain: Math.round(productivityGain * 10) / 10,
                debtReduction,
                paybackPeriodDays: Math.round(paybackPeriodDays),
                fiveYearValue: Math.round(fiveYearValue),
            },
        };
    }

    formatForProspect(roi: ROICalculation): string {
        const { inputs, outputs } = roi;

        return `
ROI ANALYSIS: CKB for ${inputs.developers} Developers

Current State:
- ${inputs.developers} developers at $${inputs.monthlySalary.toLocaleString()}/month
- ${inputs.aiCodePercentage}% of code AI-generated
- ${inputs.currentDebtHours} hours/month spent on architectural debt per developer

With CKB (${inputs.violationReduction}% violation reduction):

💰 Annual Savings: $${outputs.annualSavings.toLocaleString()}
⏱️  Time Saved: ${outputs.timeSavedHours} hours/year
📈 Productivity Gain: ${outputs.productivityGain}%
🎯 Debt Reduction: ${outputs.debtReduction}%
⚡ Payback Period: ${outputs.paybackPeriodDays} days
💎 5-Year Value: $${outputs.fiveYearValue.toLocaleString()}

This means your team gains ${Math.round(outputs.timeSavedHours / 2000)} full-time developer equivalents without hiring.

Calculate your own ROI: https://ckb.dev/roi
    `;
    }

    generatePDF(roi: ROICalculation): Uint8Array {
        // Generate PDF report for sales
        // Implementation using pdf-lib similar to invoicing
        return new Uint8Array(0);
    }
}

export const roiCalculator = new ROICalculator();
