import { PrismaClient } from '@prisma/client';

const prisma = new PrismaClient();

export interface CustomerHealth {
    userId: string;
    score: number; // 0-100
    category: 'healthy' | 'at-risk' | 'churn-risk';
    factors: {
        engagement: number;
        adoption: number;
        satisfaction: number;
        support: number;
        billing: number;
    };
    warnings: string[];
    nextActions: string[];
}

export class HealthScoreService {
    async calculateHealth(userId: string): Promise<CustomerHealth> {
        const user = await prisma.user.findUnique({
            where: { id: userId },
            include: {
                projects: true,
                scans: true,
                supportTickets: true,
                payments: true,
            } as any, // using any as these relations might not exist in the basic schema yet
        });

        if (!user) throw new Error('User not found');

        // Engagement score (30% weight)
        const engagement = await this.calculateEngagement(user);

        // Adoption score (25% weight)
        const adoption = await this.calculateAdoption(user);

        // Satisfaction score (20% weight)
        const satisfaction = await this.calculateSatisfaction(user);

        // Support score (15% weight)
        const support = await this.calculateSupport(user);

        // Billing score (10% weight)
        const billing = await this.calculateBilling(user);

        // Calculate weighted total
        const score = Math.round(
            engagement * 0.3 +
            adoption * 0.25 +
            satisfaction * 0.2 +
            support * 0.15 +
            billing * 0.1
        );

        // Determine category
        let category: 'healthy' | 'at-risk' | 'churn-risk';
        if (score >= 70) {
            category = 'healthy';
        } else if (score >= 40) {
            category = 'at-risk';
        } else {
            category = 'churn-risk';
        }

        // Generate warnings
        const warnings = this.generateWarnings({ engagement, adoption, satisfaction, support, billing });

        // Recommend next actions
        const nextActions = this.generateNextActions({ engagement, adoption, satisfaction, support, billing });

        return {
            userId,
            score,
            category,
            factors: { engagement, adoption, satisfaction, support, billing },
            warnings,
            nextActions,
        };
    }

    private async calculateEngagement(user: any): Promise<number> {
        const thirtyDaysAgo = new Date();
        thirtyDaysAgo.setDate(thirtyDaysAgo.getDate() - 30);

        // Login frequency - assuming userSession relation exists
        let logins = 0;
        try {
            logins = await (prisma as any).userSession.count({
                where: {
                    userId: user.id,
                    createdAt: { gte: thirtyDaysAgo },
                },
            });
        } catch { logins = 0; }

        // Feature usage
        const scans = user.scans ? user.scans.filter((s: any) => new Date(s.createdAt) >= thirtyDaysAgo).length : 0;

        let impactAnalyses = 0;
        try {
            impactAnalyses = await (prisma as any).impactAnalysis.count({
                where: {
                    userId: user.id,
                    createdAt: { gte: thirtyDaysAgo },
                },
            });
        } catch { impactAnalyses = 0; }

        // Time in product
        let totalTime = { _sum: { duration: 0 } };
        try {
            totalTime = await (prisma as any).userSession.aggregate({
                where: {
                    userId: user.id,
                    createdAt: { gte: thirtyDaysAgo },
                },
                _sum: { duration: true },
            });
        } catch { totalTime = { _sum: { duration: 0 } }; }

        // Score calculation
        let score = 0;
        if (logins >= 20) score += 40;
        else if (logins >= 10) score += 30;
        else if (logins >= 5) score += 20;
        else if (logins >= 1) score += 10;

        if (scans >= 10) score += 30;
        else if (scans >= 5) score += 20;
        else if (scans >= 1) score += 10;

        if (impactAnalyses >= 5) score += 30;
        else if (impactAnalyses >= 1) score += 15;

        if (totalTime._sum.duration && totalTime._sum.duration >= 3600) score += 20; // 1+ hour

        return Math.min(score, 100);
    }

    private async calculateAdoption(user: any): Promise<number> {
        // Feature adoption
        const features = [
            'cli', 'vscode', 'mcp', 'dashboard', 'api', 'ci'
        ];

        let adoptedCount = 0;
        for (const feature of features) {
            try {
                const used = await (prisma as any).featureUsage.findFirst({
                    where: {
                        userId: user.id,
                        feature,
                    },
                });
                if (used) adoptedCount++;
            } catch { /* schema might not exist */ }
        }

        return (adoptedCount / features.length) * 100;
    }

    private async calculateSatisfaction(user: any): Promise<number> {
        // NPS responses
        try {
            const nps = await (prisma as any).npsResponse.findFirst({
                where: { userId: user.id },
                orderBy: { createdAt: 'desc' },
            });

            if (nps) {
                return (nps.score / 10) * 100; // Convert 0-10 to 0-100
            }
        } catch { /* schema check */ }

        // Default to neutral
        return 50;
    }

    private async calculateSupport(user: any): Promise<number> {
        const thirtyDaysAgo = new Date();
        thirtyDaysAgo.setDate(thirtyDaysAgo.getDate() - 30);

        let tickets: any[] = [];
        try {
            tickets = await (prisma as any).supportTicket.findMany({
                where: {
                    userId: user.id,
                    createdAt: { gte: thirtyDaysAgo },
                },
            });
        } catch { return 100; }

        if (tickets.length === 0) return 100;

        // Check resolution times and satisfaction
        const resolvedTickets = tickets.filter(t => t.resolvedAt);
        const avgResolutionTime = resolvedTickets.reduce((sum, t) =>
            sum + (t.resolvedAt!.getTime() - t.createdAt.getTime()), 0
        ) / resolvedTickets.length / (1000 * 60 * 60); // hours

        let score = 100 - (tickets.length * 10);
        if (avgResolutionTime > 24) score -= 20; // > 24 hours is bad

        return Math.max(score, 0);
    }

    private async calculateBilling(user: any): Promise<number> {
        const thirtyDaysAgo = new Date();
        thirtyDaysAgo.setDate(thirtyDaysAgo.getDate() - 30);

        const failedPayments = user.payments ? user.payments.filter((p: any) =>
            p.status === 'failed' && new Date(p.createdAt) >= thirtyDaysAgo
        ).length : 0;

        const onTrial = user.subscription?.status === 'trialing';
        const pastDue = user.subscription?.status === 'past_due';

        let score = 100;
        if (failedPayments > 0) score -= failedPayments * 20;
        if (pastDue) score -= 50;
        if (onTrial) score = 75; // Trials are good but not as valuable

        return Math.max(score, 0);
    }

    private generateWarnings(factors: any): string[] {
        const warnings = [];

        if (factors.engagement < 30) {
            warnings.push('Low engagement - user hasn\'t logged in recently');
        }
        if (factors.adoption < 40) {
            warnings.push('Low feature adoption - user not using core features');
        }
        if (factors.satisfaction < 30) {
            warnings.push('Low satisfaction - recent NPS score is poor');
        }
        if (factors.support < 50) {
            warnings.push('Support issues - multiple unresolved tickets');
        }
        if (factors.billing < 50) {
            warnings.push('Billing risk - failed payments detected');
        }

        return warnings;
    }

    private generateNextActions(factors: any): string[] {
        const actions = [];

        if (factors.engagement < 30) {
            actions.push('Send re-engagement email with feature highlights');
            actions.push('Offer personalized onboarding session');
        }
        if (factors.adoption < 40) {
            actions.push('Schedule product tour of unused features');
            actions.push('Share case studies of successful users');
        }
        if (factors.satisfaction < 30) {
            actions.push('Send satisfaction survey to understand issues');
            actions.push('Schedule executive check-in call');
        }
        if (factors.support < 50) {
            actions.push('Escalate open support tickets');
            actions.push('Assign dedicated support engineer');
        }
        if (factors.billing < 50) {
            actions.push('Contact about payment issues');
            actions.push('Offer alternative payment method');
        }

        return actions.slice(0, 3); // Top 3 actions
    }

    async getAtRiskCustomers(): Promise<any[]> {
        const users = await prisma.user.findMany({
            where: { active: true } as any, // assuming active field doesn't exist yet natively
        });

        const atRisk = [];
        for (const user of users) {
            const health = await this.calculateHealth(user.id);
            if (health.category === 'churn-risk' || health.category === 'at-risk') {
                atRisk.push({
                    ...user,
                    health,
                });
            }
        }

        return atRisk;
    }
}

export const healthScoreService = new HealthScoreService();
