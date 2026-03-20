<script lang="ts">
	import { playerStore } from '$lib/stores/player.svelte';
</script>

{#if playerStore.errorMessage}
	<!-- svelte-ignore a11y_click_events_have_key_events -->
	<!-- svelte-ignore a11y_no_static_element_interactions -->
	<div class="error-toast" onclick={() => playerStore.dismissError()}>
		<svg width="14" height="14" viewBox="0 0 16 16" fill="currentColor" class="error-icon">
			<path
				d="M8 1a7 7 0 100 14A7 7 0 008 1zm-.75 3.5h1.5v5h-1.5v-5zm.75 8a1 1 0 110-2 1 1 0 010 2z"
			/>
		</svg>
		<span class="error-text">{playerStore.errorMessage}</span>
		<button class="dismiss-btn" onclick={() => playerStore.dismissError()}>
			<svg width="12" height="12" viewBox="0 0 16 16" fill="currentColor">
				<path
					d="M8 8.707l3.646 3.647.708-.707L8.707 8l3.647-3.646-.707-.708L8 7.293 4.354 3.646l-.708.708L7.293 8l-3.647 3.646.708.708L8 8.707z"
				/>
			</svg>
		</button>
	</div>
{/if}

<style>
	.error-toast {
		position: fixed;
		bottom: 84px;
		left: 50%;
		transform: translateX(-50%);
		display: flex;
		align-items: flex-start;
		gap: 8px;
		padding: 10px 14px;
		background-color: #5c1a1a;
		border: 1px solid #8b2e2e;
		border-radius: 6px;
		color: #f8d7da;
		font-size: 12px;
		z-index: 200;
		max-width: 700px;
		box-shadow: 0 4px 16px rgba(0, 0, 0, 0.4);
		cursor: pointer;
	}

	.error-icon {
		flex-shrink: 0;
		color: #e57373;
		margin-top: 1px;
	}

	.error-text {
		word-break: break-word;
		line-height: 1.4;
	}

	.dismiss-btn {
		background: none;
		border: none;
		color: #f8d7da;
		cursor: pointer;
		padding: 2px;
		flex-shrink: 0;
		opacity: 0.7;
	}

	.dismiss-btn:hover {
		opacity: 1;
	}
</style>
