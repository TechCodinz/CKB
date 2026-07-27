import express from 'express';
import { paymentService } from '../payments/stripe';
// import { authenticate } from '../middleware/auth'; // Mocked below for now
import Stripe from 'stripe';

const router = express.Router();
// Mock authenticate middleware
const authenticate = (req: any, res: any, next: any) => { req.user = { id: "user_123" }; next(); };
// Mock prisma
const prisma: any = { user: { findUnique: async () => ({ stripeCustomerId: "cus_123", subscription: { payments: [] } }) } };
const stripe = new Stripe(process.env.STRIPE_SECRET_KEY!, { apiVersion: '2023-10-16' });

// Get all plans
router.get('/plans', async (req, res) => {
    const plans = [
        {
            id: 'free',
            name: 'Free',
            price: 0,
            features: [
                '5 projects',
                'Basic scanning',
                'Community support',
            ],
        },
        {
            id: 'pro',
            name: 'Pro',
            price: 29,
            features: [
                'Unlimited projects',
                'Advanced patterns',
                'MCP integration',
                'Email support',
            ],
        },
        {
            id: 'team',
            name: 'Team',
            price: 99,
            features: [
                'Everything in Pro',
                '5 team members',
                'Team dashboard',
                'Priority support',
            ],
        },
    ];

    res.json({ plans });
});

// Create checkout session
router.post('/create-checkout', authenticate, async (req: any, res) => {
    try {
        const { planId, interval, successUrl, cancelUrl } = req.body;

        const result = await paymentService.createCheckoutSession({
            userId: req.user.id,
            planId,
            interval,
            successUrl,
            cancelUrl,
        });

        res.json(result);
    } catch (error: any) {
        res.status(400).json({ error: error.message });
    }
});

// Create customer portal
router.post('/customer-portal', authenticate, async (req: any, res) => {
    try {
        const user = await prisma.user.findUnique({
            where: { id: req.user.id },
        });

        if (!user?.stripeCustomerId) {
            res.status(400).json({ error: 'No customer found' });
            return;
        }

        const result = await paymentService.createCustomerPortal(
            user.stripeCustomerId,
            req.body.returnUrl
        );

        res.json(result);
    } catch (error: any) {
        res.status(400).json({ error: error.message });
    }
});

// Update subscription
router.post('/subscription/update', authenticate, async (req: any, res) => {
    try {
        const { subscriptionId, planId, interval } = req.body;

        const result = await paymentService.updateSubscription({
            subscriptionId,
            planId,
            interval,
        });

        res.json({ success: true, subscription: result });
    } catch (error: any) {
        res.status(400).json({ error: error.message });
    }
});

// Cancel subscription
router.post('/subscription/cancel', authenticate, async (req: any, res) => {
    try {
        const { subscriptionId, immediate } = req.body;

        const result = await paymentService.cancelSubscription(subscriptionId, immediate);

        res.json(result);
    } catch (error: any) {
        res.status(400).json({ error: error.message });
    }
});

// Get invoices
router.get('/invoices', authenticate, async (req: any, res) => {
    try {
        const user = await prisma.user.findUnique({
            where: { id: req.user.id },
            include: {
                subscription: {
                    include: {
                        payments: true,
                    },
                },
            },
        });

        res.json({ invoices: user?.subscription?.payments || [] });
    } catch (error: any) {
        res.status(400).json({ error: error.message });
    }
});

// Stripe webhook
router.post('/webhook', express.raw({ type: 'application/json' }), async (req: any, res) => {
    const sig = req.headers['stripe-signature'];

    let event;

    try {
        event = stripe.webhooks.constructEvent(
            req.body,
            sig!,
            process.env.STRIPE_WEBHOOK_SECRET!
        );
    } catch (err: any) {
        res.status(400).send(`Webhook Error: ${err.message}`);
        return;
    }

    await paymentService.handleWebhook(event);

    res.json({ received: true });
});

// Flutterwave payment initialization
router.post('/flutterwave/initialize', authenticate, async (req: any, res) => {
    try {
        const { planId, amount, currency, redirectUrl } = req.body;
        const { flutterwavePaymentService } = require('../payments/flutterwave');

        const result = await flutterwavePaymentService.initializePayment({
            userId: req.user.id,
            email: req.user.email || 'user@example.com',
            name: req.user.name || 'CKB Subscriber',
            planId: planId || 'pro',
            amount: amount || 29,
            currency: currency || 'USD',
            redirectUrl: redirectUrl || 'https://ckb.dev/dashboard',
        });

        res.json(result);
    } catch (error: any) {
        res.status(400).json({ error: error.message });
    }
});

// Flutterwave transaction verification
router.get('/flutterwave/verify/:transactionId', async (req: any, res) => {
    try {
        const { flutterwavePaymentService } = require('../payments/flutterwave');
        const result = await flutterwavePaymentService.verifyTransaction(req.params.transactionId);
        res.json(result);
    } catch (error: any) {
        res.status(400).json({ error: error.message });
    }
});

// Flutterwave webhook handler
router.post('/flutterwave/webhook', express.json(), async (req: any, res) => {
    try {
        const signature = req.headers['verif-hash'] as string;
        const { flutterwavePaymentService } = require('../payments/flutterwave');
        const result = await flutterwavePaymentService.handleWebhook(req.body, signature);
        res.json(result);
    } catch (error: any) {
        res.status(400).json({ error: error.message });
    }
});

export default router;
