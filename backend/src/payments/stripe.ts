import Stripe from 'stripe';
import { PrismaClient } from '@prisma/client';

const prisma = new PrismaClient();
const stripe = new Stripe(process.env.STRIPE_SECRET_KEY!, {
    apiVersion: '2023-10-16',
});

export class PaymentService {
    // Create checkout session
    async createCheckoutSession({
        userId,
        planId,
        interval = 'month',
        successUrl,
        cancelUrl,
    }: {
        userId: string;
        planId: string;
        interval?: 'month' | 'year';
        successUrl: string;
        cancelUrl: string;
    }) {
        const user = await prisma.user.findUnique({ where: { id: userId } });
        const plan = await prisma.plan.findUnique({ where: { id: planId } });

        if (!user || !plan) throw new Error('User or plan not found');

        const priceId = interval === 'month' ? plan.stripePriceIdMonth : plan.stripePriceIdYear;

        const session = await stripe.checkout.sessions.create({
            customer: user.stripeCustomerId,
            mode: 'subscription',
            line_items: [{ price: priceId, quantity: 1 }],
            success_url: successUrl,
            cancel_url: cancelUrl,
            metadata: {
                userId,
                planId,
            },
            subscription_data: {
                metadata: {
                    userId,
                    planId,
                },
                trial_period_days: plan.trialDays || 14,
            },
            allow_promotion_codes: true,
            billing_address_collection: 'required',
            payment_method_types: ['card'],
        });

        return { sessionId: session.id, url: session.url };
    }

    // Handle webhooks
    async handleWebhook(event: Stripe.Event) {
        switch (event.type) {
            case 'checkout.session.completed':
                await this.handleCheckoutCompleted(event.data.object as Stripe.Checkout.Session);
                break;

            case 'customer.subscription.updated':
                await this.handleSubscriptionUpdated(event.data.object as Stripe.Subscription);
                break;

            case 'customer.subscription.deleted':
                await this.handleSubscriptionDeleted(event.data.object as Stripe.Subscription);
                break;

            case 'invoice.payment_succeeded':
                await this.handlePaymentSucceeded(event.data.object as Stripe.Invoice);
                break;

            case 'invoice.payment_failed':
                await this.handlePaymentFailed(event.data.object as Stripe.Invoice);
                break;
        }
    }

    private async handleCheckoutCompleted(session: Stripe.Checkout.Session) {
        const { userId, planId } = session.metadata!;

        await prisma.user.update({
            where: { id: userId },
            data: {
                stripeCustomerId: session.customer as string,
                subscription: {
                    create: {
                        stripeSubscriptionId: session.subscription as string,
                        planId,
                        status: 'active',
                        currentPeriodStart: new Date(),
                        currentPeriodEnd: new Date(Date.now() + 30 * 24 * 60 * 60 * 1000),
                    },
                },
            },
        });

        // Send welcome email
        await this.sendWelcomeEmail(userId);

        // Track conversion
        await this.trackConversion(userId, planId);
    }

    private async handleSubscriptionUpdated(subscription: Stripe.Subscription) {
        await prisma.subscription.update({
            where: { stripeSubscriptionId: subscription.id },
            data: {
                status: subscription.status,
                currentPeriodStart: new Date(subscription.current_period_start * 1000),
                currentPeriodEnd: new Date(subscription.current_period_end * 1000),
                cancelAtPeriodEnd: subscription.cancel_at_period_end,
            },
        });
    }

    private async handleSubscriptionDeleted(subscription: Stripe.Subscription) {
        await prisma.subscription.update({
            where: { stripeSubscriptionId: subscription.id },
            data: {
                status: 'canceled',
                canceledAt: new Date(),
            },
        });

        // Send cancellation survey
        await this.sendCancellationSurvey(subscription.metadata?.userId || "");
    }

    private async handlePaymentSucceeded(invoice: Stripe.Invoice) {
        const subscriptionId = invoice.subscription as string;

        await prisma.payment.create({
            data: {
                stripeInvoiceId: invoice.id,
                subscriptionId,
                amount: invoice.amount_paid,
                currency: invoice.currency,
                status: 'succeeded',
                paidAt: new Date(),
            },
        });
    }

    private async handlePaymentFailed(invoice: Stripe.Invoice) {
        const subscriptionId = invoice.subscription as string;

        await prisma.payment.create({
            data: {
                stripeInvoiceId: invoice.id,
                subscriptionId,
                amount: invoice.amount_due,
                currency: invoice.currency,
                status: 'failed',
            },
        });

        // Send payment failed email
        const subscription = await prisma.subscription.findUnique({
            where: { stripeSubscriptionId: subscriptionId },
            include: { user: true },
        });

        if (subscription && subscription.user) {
            await this.sendPaymentFailedEmail(subscription.user.email);
        }
    }

    // Create customer portal
    async createCustomerPortal(customerId: string, returnUrl: string) {
        const session = await stripe.billingPortal.sessions.create({
            customer: customerId,
            return_url: returnUrl,
        });

        return { url: session.url };
    }

    // Update subscription
    async updateSubscription({
        subscriptionId,
        planId,
        interval = 'month',
    }: {
        subscriptionId: string;
        planId: string;
        interval?: 'month' | 'year';
    }) {
        const subscription = await prisma.subscription.findUnique({
            where: { id: subscriptionId },
        });

        if (!subscription) throw new Error('Subscription not found');

        const plan = await prisma.plan.findUnique({ where: { id: planId } });
        if (!plan) throw new Error('Plan not found');

        const priceId = interval === 'month' ? plan.stripePriceIdMonth : plan.stripePriceIdYear;

        if (!subscription.stripeSubscriptionItemId) {
            throw new Error('No subscription item found');
        }

        const updated = await stripe.subscriptions.update(subscription.stripeSubscriptionId, {
            items: [{ id: subscription.stripeSubscriptionItemId, price: priceId }],
            proration_behavior: 'always_invoice',
        });

        return updated;
    }

    // Cancel subscription
    async cancelSubscription(subscriptionId: string, cancelImmediately = false) {
        const subscription = await prisma.subscription.findUnique({
            where: { id: subscriptionId },
        });

        if (!subscription) throw new Error('Subscription not found');

        if (cancelImmediately) {
            await stripe.subscriptions.cancel(subscription.stripeSubscriptionId);
        } else {
            await stripe.subscriptions.update(subscription.stripeSubscriptionId, {
                cancel_at_period_end: true,
            });
        }

        return { success: true };
    }

    // Private helper methods
    private async sendWelcomeEmail(userId: string) {
        // Implement email sending
        console.log(`Sending welcome email to user ${userId}`);
    }

    private async sendCancellationSurvey(userId: string) {
        // Implement cancellation survey
        console.log(`Sending cancellation survey to user ${userId}`);
    }

    private async sendPaymentFailedEmail(email: string) {
        // Implement payment failed email
        console.log(`Sending payment failed email to ${email}`);
    }

    private async trackConversion(userId: string, planId: string) {
        // Track in analytics
        console.log(`Conversion: user ${userId} -> plan ${planId}`);
    }
}

export const paymentService = new PaymentService();
