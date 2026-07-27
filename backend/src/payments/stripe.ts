import Stripe from 'stripe';
import { PrismaClient } from '@prisma/client';

const prisma = new PrismaClient();
const stripe = new Stripe(process.env.STRIPE_SECRET_KEY || 'sk_test_mock', {
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

        if (!user) throw new Error('User not found');

        const priceEnvKey = `STRIPE_PRICE_${planId.toUpperCase()}_${interval.toUpperCase()}`;
        const priceId = process.env[priceEnvKey] || 'price_pro_monthly';

        const session = await stripe.checkout.sessions.create({
            customer: user.stripeCustomerId || undefined,
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
                trial_period_days: 14,
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
                plan: planId || 'pro',
                subscription: {
                    create: {
                        stripeSubscriptionId: session.subscription as string,
                        planId: planId || 'pro',
                        status: 'active',
                        currentPeriodStart: new Date(),
                        currentPeriodEnd: new Date(Date.now() + 30 * 24 * 60 * 60 * 1000),
                    },
                },
            },
        });

        console.log(`✅ Stripe Subscription Created for User ${userId} (Plan: ${planId})`);
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
    }

    private async handlePaymentSucceeded(invoice: Stripe.Invoice) {
        const subscriptionId = invoice.subscription as string;
        const subscription = await prisma.subscription.findUnique({
            where: { stripeSubscriptionId: subscriptionId },
        });

        if (subscription) {
            await prisma.payment.create({
                data: {
                    userId: subscription.userId,
                    stripeInvoiceId: invoice.id,
                    subscriptionId: subscription.id,
                    amount: invoice.amount_paid,
                    currency: invoice.currency,
                    status: 'succeeded',
                    paidAt: new Date(),
                },
            });
        }
    }

    private async handlePaymentFailed(invoice: Stripe.Invoice) {
        const subscriptionId = invoice.subscription as string;
        const subscription = await prisma.subscription.findUnique({
            where: { stripeSubscriptionId: subscriptionId },
        });

        if (subscription) {
            await prisma.payment.create({
                data: {
                    userId: subscription.userId,
                    stripeInvoiceId: invoice.id,
                    subscriptionId: subscription.id,
                    amount: invoice.amount_due,
                    currency: invoice.currency,
                    status: 'failed',
                },
            });
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

        if (!subscription || !subscription.stripeSubscriptionId) {
            throw new Error('Subscription not found');
        }

        const priceEnvKey = `STRIPE_PRICE_${planId.toUpperCase()}_${interval.toUpperCase()}`;
        const priceId = process.env[priceEnvKey] || 'price_pro_monthly';

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

        if (!subscription || !subscription.stripeSubscriptionId) {
            throw new Error('Subscription not found');
        }

        if (cancelImmediately) {
            await stripe.subscriptions.cancel(subscription.stripeSubscriptionId);
        } else {
            await stripe.subscriptions.update(subscription.stripeSubscriptionId, {
                cancel_at_period_end: true,
            });
        }

        return { success: true };
    }
}

export const paymentService = new PaymentService();
