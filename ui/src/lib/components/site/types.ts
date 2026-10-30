export type SiteDetail = {
	site: {
		id: number;
		slug: string;
		kind: string;
		template: string;
		status: string;
		staging: boolean;
		log_retention_days: number;
		php_version: string;
		settings: { memory_limit_mb: number; upload_max_mb: number; max_execution_time_s: number; max_children: number };
	};
	domains: { hostname: string; kind: 'primary' | 'alias' | 'redirect' }[];
	can_manage: boolean;
	staging: { user: string; password: string } | null;
	wp_admin_pending?: boolean;
	php_runtimes: string[];
};
